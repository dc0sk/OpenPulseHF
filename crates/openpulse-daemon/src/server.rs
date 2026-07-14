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
    apply_command_to_engine, check_ptt_watchdog, expire_pending_handshake,
    maybe_qsy_on_interference, ota_status_event, process_received_bytes, ws, ControlServer,
    RuntimeControlState,
};
use openpulse_audio::LoopbackBackend;
use openpulse_config::OpenpulseConfig;
use openpulse_core::audio::{AudioBackend, AudioInputStream};
use openpulse_core::relay::{RelayForwarder, RelayTrustPolicy};
use openpulse_core::station_id::StationIdTimer;
use openpulse_core::trust_store_file::load_trust_store_from_file;
use openpulse_modem::ModemEngine;
use openpulse_qsy::session::QsyPolicy;
use openpulse_radio::{
    CatController, NoOpPtt, PttController, RigctldController, RigctldPtt, VoxPtt,
};
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
    // Opt-in end-to-end session compression: pack OTA data payloads before TX. The RX side always
    // unpacks a self-describing frame regardless of this flag (see the rx tick), so it is safe on one end.
    let compress_tx = cfg.compression.enabled;
    let initial_bandplan_mode = if cfg.qsy.bandplan_awareness_enabled {
        cfg.qsy.bandplan_mode.clone()
    } else {
        "unrestricted".to_string()
    };

    let mut engine = ModemEngine::new(modem_backend);
    // Record the operator's identity + declared TX power in the §97 regulatory TX-metadata log; without
    // this the log stamps an empty callsign / 0 W on every frame (set_callsign is otherwise only wired
    // from two CLI subcommands).
    engine.set_callsign(cfg.station.callsign.clone());
    engine.set_max_power_watts(cfg.station.tx_power_watts);
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

    engine.set_cessb_enabled(cfg.modem.cessb_enabled);
    tracing::info!(cessb = cfg.modem.cessb_enabled, "CE-SSB TX conditioning");

    // Receiver-side automatic notch for out-of-band CW interference (opt-in).
    if cfg.modem.notch_enabled {
        engine.configure_notch(cfg.modem.notch_max, cfg.modem.notch_q, 2000.0);
        engine.set_notch_persistence(cfg.modem.notch_persistence);
        engine.enable_notch();
    }
    tracing::info!(
        notch = cfg.modem.notch_enabled,
        max = cfg.modem.notch_max,
        q = cfg.modem.notch_q,
        persistence = cfg.modem.notch_persistence,
        "receiver auto-notch"
    );

    // Auto-QSY on a confirmed in-band interferer needs notch persistence to populate the hint.
    let qsy_auto_on_interference = cfg.qsy.auto_qsy_on_interference;
    if qsy_auto_on_interference && !(cfg.modem.notch_enabled && cfg.modem.notch_persistence > 0) {
        tracing::warn!(
            "qsy.auto_qsy_on_interference is set but requires [modem] notch_enabled = true and \
             notch_persistence > 0 to detect in-band interferers; it will not trigger"
        );
    }

    // Our active OTA ladder identity `(name, fingerprint)`, advertised in the signed handshake so a
    // peer running a diverged ladder is detected (then OTA is suppressed). `None` when OTA is off.
    let mut ota_ladder_identity: Option<(String, u64)> = None;
    // Receiver-led OTA adaptive rate-stepping (opt-in via [modem] ota_enabled).
    if cfg.modem.ota_enabled {
        let profile_name = if cfg.modem.ota_profile.is_empty() {
            cfg.modem.profile.as_str()
        } else {
            cfg.modem.ota_profile.as_str()
        };
        match openpulse_core::profile::SessionProfile::by_name(profile_name) {
            Some(profile) => {
                // Ladder identity for backward-compat: two stations must run the same (mode, FEC)
                // mapping for `recommended_level` to mean the same thing. The fingerprint captures
                // that mapping (not local floors); operators can diff it across stations, and the
                // handshake guard (follow-up) will negotiate it. See docs/dev/design/ladder-versioning.md.
                let fingerprint = profile.fingerprint();
                ota_ladder_identity = Some((profile_name.to_string(), fingerprint));
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
                // The aggressiveness preset, when set, sets both A2/A3 gates and so
                // takes precedence over the individual knobs above.
                if !cfg.modem.ota_aggressiveness.is_empty() {
                    match openpulse_core::rate::OtaAggressiveness::from_name(
                        &cfg.modem.ota_aggressiveness,
                    ) {
                        Some(p) => {
                            engine.set_ota_aggressiveness(p);
                            tracing::info!(preset = p.name(), "OTA aggressiveness preset applied");
                        }
                        None => tracing::warn!(
                            value = %cfg.modem.ota_aggressiveness,
                            "unknown ota_aggressiveness preset; ignored"
                        ),
                    }
                }
                tracing::info!(
                    profile = profile_name,
                    ladder_fingerprint = format!("{fingerprint:016x}"),
                    "OTA adaptive rate-stepping enabled"
                );
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
            callsign: cfg.station.callsign.clone(),
            id_interval_secs: cfg.station.auto_id_interval_secs,
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

    // Control-channel auth (REQ-SEC-CTL-01/02): required on a non-loopback bind, or when configured.
    // Fail closed — refuse to start if auth is required but no PSK is provided.
    let require_auth = openpulse_linksec::auth_required(
        &cfg.daemon.tcp_bind_addr,
        cfg.control_security.require_auth,
    );
    let control_psk = match load_control_psk() {
        Ok(psk) => psk,
        Err(e) => return Err(e),
    };
    if require_auth && control_psk.is_none() {
        return Err(format!(
            "control channel requires authentication (bind {} / require_auth={}) but no PSK is set — \
             set OPENPULSE_CONTROL_PSK to 64 hex chars (32 bytes)",
            cfg.daemon.tcp_bind_addr, cfg.control_security.require_auth
        ));
    }
    let control_psk = if require_auth { control_psk } else { None };
    if control_psk.is_some() {
        tracing::info!("control channel: PSK authentication + encryption enabled (Noise)");
    }

    let mut handle = ControlServer::spawn(
        tcp_bind,
        &engine,
        crate::ControlServerConfig {
            initial_mode: mode,
            initial_station_id: station_id,
            initial_qsy_enabled,
            initial_bandplan_mode,
            initial_allow_tuner_on_high_swr,
            control_psk,
        },
        None,
    )
    .await
    .map_err(|e| format!("failed to bind TCP control port {tcp_bind}: {e}"))?;

    tracing::info!("openpulse-server TCP control port listening on {tcp_bind}");

    // The WebSocket control endpoint carries the *same* command protocol as the TCP port (PttAssert,
    // SendMessage, EnableRepeater, …) but has no authentication path. Fail closed: if auth is required
    // for either bind, do NOT spawn the unauthenticated WS listener — otherwise it would bypass the auth
    // the TCP port enforces (REQ-SEC-CTL-02). WS auth (Noise-over-WS) is a documented follow-up.
    let ws_auth_required = ws_disabled_for_auth(require_auth, &cfg.daemon.websocket_bind_addr);
    if ws_auth_required {
        tracing::warn!(
            ws_bind = %ws_bind,
            "WebSocket control port DISABLED: control auth is required but the WS endpoint cannot \
             authenticate. Use the TCP control port (Noise/PSK), or bind both to loopback."
        );
    } else {
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
                valid_modes: handle.valid_modes.clone(),
            },
            None,
        )
        .await
        .map_err(|e| format!("failed to bind WebSocket control port {ws_bind}: {e}"))?;
        tracing::info!("openpulse-server WebSocket control port listening on {ws_bind}");
    }

    // Audit mode (REQ-OBS-01): write a startup snapshot, then record the control-event stream to
    // <archive_dir>/events.ndjson, tapping the same broadcast channel clients subscribe to — no
    // live client required.
    if cfg.observability.audit_mode {
        let dir = openpulse_config::logging::expand_tilde(&cfg.observability.archive_dir);
        if let Err(e) = crate::audit::write_startup_snapshot(&dir, &cfg) {
            tracing::warn!(error = %e, "audit: failed to write snapshot.json");
        }
        crate::audit::spawn_event_recorder(dir, handle.event_tx.subscribe());
    }

    // CAT backend selection. "none" runs with no CAT control for a TRX that
    // rigctld/Hamlib does not support — no connection is attempted, the operator
    // tunes manually, and frequency-control commands are rejected. PTT is
    // independent (see build_ptt_controller / [modem] ptt_backend).
    let mut rig_controller = build_cat_controller(&cfg.radio);

    // Live rig-meter poll task (operator drive-tuning aid): a *dedicated* rigctld
    // connection polls ALC / power-out / SWR and emits `RigStatus` events so the
    // panel can show live ALC while the operator sets drive. The separate
    // connection means it never contends with the PTT/frequency command path.
    // `[radio] meter_poll_ms = 0` disables it.
    if !cfg.radio.cat_backend.eq_ignore_ascii_case("none") && cfg.radio.meter_poll_ms > 0 {
        match RigctldController::connect(&cfg.radio.rigctld_addr) {
            Ok(mut poll_rig) => {
                let ev = handle.event_tx.clone();
                let interval = std::time::Duration::from_millis(cfg.radio.meter_poll_ms);
                tokio::task::spawn_blocking(move || {
                    use crate::protocol::ControlEvent;
                    let mut freq = poll_rig.get_frequency().unwrap_or(0);
                    let mut mode = poll_rig
                        .get_mode()
                        .map(|m| m.as_str().to_string())
                        .unwrap_or_default();
                    let mut tick: u32 = 0;
                    loop {
                        std::thread::sleep(interval);
                        // Frequency/mode change rarely — refresh ~every 10 cycles;
                        // poll the meters every cycle.
                        if tick.is_multiple_of(10) {
                            if let Ok(f) = poll_rig.get_frequency() {
                                freq = f;
                            }
                            if let Ok(m) = poll_rig.get_mode() {
                                mode = m.as_str().to_string();
                            }
                        }
                        tick = tick.wrapping_add(1);
                        let _ = ev.send(ControlEvent::RigStatus {
                            rig: "rigctld".into(),
                            freq_hz: freq,
                            mode: mode.clone(),
                            power_w: poll_rig.get_power_out().ok(),
                            alc: poll_rig.get_alc().ok(),
                            swr: poll_rig.get_swr().ok(),
                        });
                    }
                });
                tracing::info!(
                    interval_ms = cfg.radio.meter_poll_ms,
                    "rig meter poll task started (live ALC/power/SWR)"
                );
            }
            Err(err) => tracing::warn!(
                addr = %cfg.radio.rigctld_addr,
                error = %err,
                "rig meter poll: second rigctld connection failed; live meters disabled"
            ),
        }
    }

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

    // Station identity seed for signing handshake (CONREQ/CONACK) frames. An explicit path lets
    // co-located stations (the twin rig) hold distinct identities; empty uses the platform default.
    let station_seed = {
        let loaded = if cfg.station.identity_key_path.is_empty() {
            openpulse_config::load_or_generate_identity()
        } else {
            openpulse_config::load_identity_from(std::path::Path::new(
                &cfg.station.identity_key_path,
            ))
        };
        match loaded {
            Ok(seed) => seed,
            Err(e) => {
                tracing::warn!(error = %e, "failed to load station identity key; handshake frames will use an ephemeral key");
                let mut seed = [0u8; 32];
                use rand::RngCore;
                rand::rngs::OsRng.fill_bytes(&mut seed);
                seed
            }
        }
    };

    let mut runtime_state = RuntimeControlState {
        repeater_enabled: cfg.repeater.enabled,
        repeater: Some(repeater),
        station_seed,
        local_callsign: cfg.station.callsign.clone(),
        local_grid: cfg.station.grid_square.clone(),
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
        dcd_squelch_default: cfg.modem.dcd_squelch,
        dcd_squelch_bands: cfg.modem.dcd_squelch_bands.clone(),
        local_ota_ladder: ota_ladder_identity,
        compress_tx,
        filexfer_policy: crate::filexfer::FileTransferPolicy::from_config(&cfg.file_transfer),
        logbook: crate::logbook::Logbook::new(
            cfg.logbook.enabled,
            &cfg.logbook.adif_path,
            &cfg.station.callsign,
            &cfg.station.grid_square,
            &cfg.logbook.peer_grids,
        ),
        discovery: build_discovery_runtime(&cfg),
        discovery_calling_freqs_hz: cfg.discovery.calling_freqs_hz.clone(),
        discovery_rendezvous_channels_hz: cfg.discovery.rendezvous_channels_hz.clone(),
        ..RuntimeControlState::default()
    };
    validate_rendezvous_channels(&cfg);
    if cfg.logbook.enabled {
        tracing::info!(path = %cfg.logbook.adif_path, "ADIF logbook enabled");
    }

    // Apply the default DCD squelch at startup; per-band overrides kick in on retune.
    engine.set_dcd_squelch(cfg.modem.dcd_squelch);

    // Execute side-effectful commands against the live modem engine.
    // The receive ticker polls the modem for decoded bytes so the QSY responder path
    // can react to incoming RF frames without operator commands.
    let mut rx_ticker =
        tokio::time::interval(std::time::Duration::from_millis(cfg.daemon.receive_tick_ms));
    // Safety-critical PTT watchdog on its own fast timer + `select!` arm, so a client command flood can
    // no longer starve the transmitter's force-release along with the rx tick (audit robustness item):
    // the watchdog is decoupled from the rx decode, and the loop is no longer `biased` toward commands.
    let mut watchdog_ticker = tokio::time::interval(std::time::Duration::from_millis(100));
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
    // Hold ONE capture stream open across receive ticks: cpal is a callback backend
    // whose buffer only fills while the stream is held open, so reopening it every
    // tick (~20 Hz) never warms up on real hardware and decodes nothing. Opened
    // lazily and reopened on read error (e.g. device unplugged); a LoopbackBackend
    // stream clones shared buffers, so this is equivalent to per-tick reopen there.
    let capture_device = (!cfg.audio.device.is_empty()).then(|| cfg.audio.device.clone());
    let mut rx_stream: Option<Box<dyn AudioInputStream>> = None;
    // Periodic station identification (REQ-REG-10): while transmitting, key up and send the
    // callsign at least every `auto_id_interval_secs`. The pure `StationIdTimer` is fed a
    // monotonic ms clock (`id_start`) and armed by polling the engine's `frames_transmitted`
    // delta, so no `note_tx()` call has to be threaded through every transmit site. Disabled
    // when the interval is 0 or the callsign is unset/default (never auto-ID as N0CALL).
    let id_start = std::time::Instant::now();
    let mut id_timer =
        StationIdTimer::new(cfg.station.auto_id_interval_secs.saturating_mul(1000), 0)
            .with_signoff_idle_ms(cfg.station.auto_id_signoff_idle_secs.saturating_mul(1000));
    let id_callsign = cfg.station.callsign.trim().to_string();
    let auto_id_active = id_timer.is_enabled()
        && !id_callsign.is_empty()
        && !id_callsign.eq_ignore_ascii_case("N0CALL");
    let mut tx_frames_seen = engine.frames_transmitted();
    if auto_id_active {
        tracing::info!(
            interval_s = cfg.station.auto_id_interval_secs,
            callsign = %id_callsign,
            "periodic station ID enabled"
        );
    }
    loop {
        tokio::select! {
            // No `biased`: fair scheduling so a command flood cannot starve the rx tick or the watchdog.
            _ = watchdog_ticker.tick() => {
                release_ptt_on_watchdog(&mut runtime_state, &handle.event_tx, &mut ptt_controller);
            }
            Some(cmd) = handle.commands.recv() => {
                // PTT hardware calls are synchronous; handle them before the async engine dispatch so
                // the borrow of ptt_controller doesn't cross the await point.
                // If the hardware call fails, skip the engine dispatch to avoid emitting a spurious
                // PttChanged event that would tell clients PTT is active when it is not.
                let ptt_hard_failed = handle_ptt_command(&cmd, &mut ptt_controller);
                // OTA ISS send with real-radio PTT turnaround: when a session is
                // active, a SendMessage drives the receiver-led OTA send here (where
                // the PTT controller lives) — key PTT for the data frame, release it,
                // then listen for the peer's ACK and adopt its recommendation. Handled
                // here rather than in apply_command_to_engine so PTT is sequenced
                // around the half-duplex turnaround.
                let mut ota_send_handled = false;
                if let crate::Command::SendMessage { body, .. } = &cmd {
                    // Suppress adaptive OTA (fixed-mode fallback) when a verified peer's rate ladder
                    // differs from ours — a `recommended_level` would otherwise mean different modes.
                    if engine.ota_active() && !runtime_state.ota_suppressed_by_peer() {
                        ota_send_handled = true;
                        // Compress the session payload on the wire when enabled; the peer's rx tick
                        // unpacks the self-describing frame. Falls back to raw bytes when disabled.
                        let payload = if compress_tx {
                            openpulse_core::compression::pack(body.as_bytes())
                        } else {
                            body.as_bytes().to_vec()
                        };
                        ota_send_with_ptt(
                            &mut engine,
                            &mut ptt_controller,
                            &mut runtime_state.ptt_asserted_at,
                            &handle.event_tx,
                            &payload,
                        );
                    }
                }
                if !ptt_hard_failed && !ota_send_handled {
                    apply_command_to_engine(
                        &cmd,
                        &mut engine,
                        &handle.active_mode,
                        &handle.event_tx,
                        rig_controller.as_mut().map(|c| c as &mut (dyn CatController + Send)),
                        &mut runtime_state,
                    )
                    .await;
                }
                // A `SendFile` / `AcceptFile` queued file-transfer frames — send them PTT-keyed.
                drain_filexfer_tx(
                    &mut engine,
                    &mut ptt_controller,
                    &handle.event_tx,
                    &mut runtime_state,
                );
            }
            _ = rx_ticker.tick() => {
                // Belt-and-suspenders: also check the watchdog on the rx tick (idempotent — it fires
                // once when the deadline passes). The dedicated `watchdog_ticker` arm is the primary,
                // flood-proof path.
                release_ptt_on_watchdog(&mut runtime_state, &handle.event_tx, &mut ptt_controller);
                let mode = handle.active_mode.lock().await.clone();
                // block_in_place: engine capture/transmit are synchronous; LoopbackBackend
                // returns immediately. A real audio backend blocks until samples arrive.
                let decode_start = std::time::Instant::now();
                // Accumulate a full burst before decoding: on a streaming (cpal) backend
                // one frame spans many tick windows, so decoding a single partial window
                // can't acquire it. Read the held-open capture stream and accumulate;
                // accumulate_capture returns Some only when the carrier drops.
                // Tee this tick's raw audio to the JS8 discovery dwell buffer when parked on the JS8
                // calling channel (the DCD-burst pipeline can't carry −24 dB signals; §6.2).
                let disco_dwelling = discovery_is_dwelling(&runtime_state);
                let mut discovery_raw: Vec<f32> = Vec::new();
                let burst = tokio::task::block_in_place(|| {
                    if rx_stream.is_none() {
                        rx_stream = Some(engine.open_capture_stream(capture_device.as_deref())?);
                    }
                    let read = match rx_stream.as_mut() {
                        Some(s) => s.read(),
                        None => return Ok(None),
                    };
                    match read {
                        Ok(samples) => {
                            if disco_dwelling {
                                discovery_raw = samples.clone();
                            }
                            engine.accumulate_capture(Some(&mode), samples)
                        }
                        Err(e) => {
                            // Drop the stream so the next tick reopens it.
                            rx_stream = None;
                            tracing::debug!(error = %e, "capture read failed; reopening");
                            Ok(None)
                        }
                    }
                });
                let bytes = match burst {
                    Ok(Some(burst)) if engine.ota_active() && !runtime_state.ota_suppressed_by_peer() => {
                        // Receiver-led OTA: decode the burst, then key PTT only to answer
                        // with the ACK carrying our absolute recommended_level.
                        match tokio::task::block_in_place(|| {
                            engine.ota_decode_burst(&burst, &ota_session_id)
                        }) {
                            Ok(res) => {
                                let mut keyed = true;
                                if let Some(ref mut ptt) = ptt_controller {
                                    if let Err(e) = ptt.assert_ptt() {
                                        tracing::warn!("OTA ACK PTT assert failed: {e}");
                                        keyed = false;
                                    }
                                }
                                if keyed {
                                    runtime_state.ptt_asserted_at =
                                        Some(std::time::Instant::now()); // arm watchdog
                                    let _ = handle.event_tx.send(
                                        crate::protocol::ControlEvent::PttChanged { active: true },
                                    );
                                    if let Err(e) = tokio::task::block_in_place(|| {
                                        engine.transmit_ack_with_short_fec(&res.ack, None)
                                    }) {
                                        tracing::warn!("OTA ACK transmit failed: {e}");
                                    }
                                    if let Some(ref mut ptt) = ptt_controller {
                                        if let Err(e) = ptt.release_ptt() {
                                            tracing::warn!("OTA ACK PTT release failed: {e}");
                                        } else {
                                            runtime_state.ptt_asserted_at = None;
                                        }
                                    } else {
                                        runtime_state.ptt_asserted_at = None;
                                    }
                                    let _ = handle.event_tx.send(
                                        crate::protocol::ControlEvent::PttChanged { active: false },
                                    );
                                }
                                res.payload.unwrap_or_default()
                            }
                            Err(e) => {
                                tracing::debug!("OTA burst decode error: {e}");
                                Vec::new()
                            }
                        }
                    }
                    Ok(Some(burst)) => {
                        tokio::task::block_in_place(|| engine.decode_burst(&mode, &burst))
                            .unwrap_or_default()
                    }
                    Ok(None) => Vec::new(),
                    Err(e) => {
                        tracing::debug!("RX capture error: {e}");
                        Vec::new()
                    }
                };
                // End-to-end session compression: a peer that packed its payload sent a self-describing
                // frame; unpack it here so routing, metrics, and message surfacing see the original bytes.
                // Non-packed frames (control frames, un-packed data) lack the magic and pass through.
                let bytes = openpulse_core::compression::unpack(&bytes).unwrap_or(bytes);
                let decode_ms = decode_start.elapsed().as_secs_f32() * 1000.0;
                if !bytes.is_empty() {
                    process_received_bytes(
                        &bytes,
                        &mut runtime_state,
                        rig_controller.as_mut().map(|c| c as &mut (dyn CatController + Send)),
                        &handle.event_tx,
                        &handle.active_mode,
                        &mut engine,
                    )
                    .await;
                    // The receive handler may have queued FileAccept/BlockAck/FileComplete, or an
                    // inbound ACK may have queued the next send burst — send them PTT-keyed.
                    drain_filexfer_tx(
                        &mut engine,
                        &mut ptt_controller,
                        &handle.event_tx,
                        &mut runtime_state,
                    );
                }
                // Auto-QSY if the notch persistence tracker confirmed an in-band interferer this
                // tick (one a notch can't remove). Runs every tick — interference shows during
                // silence too — and self-gates on config / candidates / an in-flight session.
                maybe_qsy_on_interference(
                    qsy_auto_on_interference,
                    &mut runtime_state,
                    rig_controller.as_mut().map(|c| c as &mut (dyn CatController + Send)),
                    &handle.event_tx,
                    &handle.active_mode,
                    &mut engine,
                )
                .await;
                // Abandon a signed handshake whose CONACK never arrived (timeout).
                expire_pending_handshake(&mut runtime_state, &handle.event_tx);
                // JS8 discovery (FF-15): feed the idle predicate + dwell audio, run the slot scheduler,
                // and execute any retune / station-heard outcomes.
                let due_beacon = discovery_tick(
                    &mut runtime_state,
                    &engine,
                    rig_controller.as_mut().map(|c| c as &mut (dyn CatController + Send)),
                    &handle.event_tx,
                    &discovery_raw,
                    epoch_ms(),
                );
                // A due beacon frame is transmitted here, where the PTT controller + `&mut engine`
                // live (half-duplex: key PTT, emit, release).
                if let Some((audio, mode)) = due_beacon {
                    transmit_beacon_with_ptt(
                        &mut engine,
                        &mut ptt_controller,
                        &mut runtime_state.ptt_asserted_at,
                        &audio,
                        &mode,
                    );
                }
                // A completed rendezvous QSY hands off to the signed session on the agreed channel: run
                // the same begin_secure_session + CONREQ path as an operator `ConnectPeer` (needs
                // `&mut engine` + the CAT rig, both owned here).
                if let Some(cmd) = take_rendezvous_connect(&mut runtime_state) {
                    apply_command_to_engine(
                        &cmd,
                        &mut engine,
                        &handle.active_mode,
                        &handle.event_tx,
                        rig_controller.as_mut().map(|c| c as &mut (dyn CatController + Send)),
                        &mut runtime_state,
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
                        // Live compressibility of the decoded payload stream: the session compressor's
                        // best-effort size (never larger than raw) drives the reported compress_ratio.
                        let (compressed, _algo) =
                            openpulse_core::compression::compress_if_smaller(&bytes);
                        m.raw_payload_bytes += bytes.len() as u64;
                        m.compressed_payload_bytes += compressed.len() as u64;
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
                // Station ID (REQ-REG-10). Arm from the TX-frame delta (any data/ACK/retransmit we
                // emitted since the last poll), then key PTT and send the callsign in the active mode
                // when either trigger is due: the 10-min *interval* ID during a communication, or the
                // *sign-off* ID once the channel has gone quiet at the end of one. Re-baseline the
                // counter afterwards so the ID frame itself is not counted as further TX activity.
                if auto_id_active {
                    let now_ms = id_start.elapsed().as_millis() as u64;
                    let tx_now = engine.frames_transmitted();
                    if tx_now != tx_frames_seen {
                        id_timer.note_tx(now_ms);
                        tx_frames_seen = tx_now;
                    }
                    let id_reason = if id_timer.id_due(now_ms) {
                        Some("interval")
                    } else if id_timer.signoff_due(now_ms) {
                        Some("sign-off")
                    } else {
                        None
                    };
                    if let Some(reason) = id_reason {
                        let id_mode = handle.active_mode.lock().await.clone();
                        let id_body = format!("DE {id_callsign}");
                        let mut keyed = true;
                        if let Some(ref mut ptt) = ptt_controller {
                            if let Err(e) = ptt.assert_ptt() {
                                tracing::warn!(error = %e, "station-ID PTT assert failed");
                                keyed = false;
                            }
                        }
                        if keyed {
                            runtime_state.ptt_asserted_at = Some(std::time::Instant::now()); // arm watchdog
                            let _ = handle
                                .event_tx
                                .send(crate::protocol::ControlEvent::PttChanged { active: true });
                            match tokio::task::block_in_place(|| {
                                engine.transmit(id_body.as_bytes(), &id_mode, None)
                            }) {
                                Ok(()) => tracing::info!(
                                    callsign = %id_callsign,
                                    mode = %id_mode,
                                    kind = reason,
                                    "transmitted station ID"
                                ),
                                Err(e) => tracing::warn!(
                                    error = %e, mode = %id_mode, "station-ID transmit failed"
                                ),
                            }
                            if let Some(ref mut ptt) = ptt_controller {
                                if let Err(e) = ptt.release_ptt() {
                                    tracing::warn!(error = %e, "station-ID PTT release failed");
                                } else {
                                    runtime_state.ptt_asserted_at = None; // released ok → disarm
                                }
                            } else {
                                runtime_state.ptt_asserted_at = None;
                            }
                            let _ = handle
                                .event_tx
                                .send(crate::protocol::ControlEvent::PttChanged { active: false });
                        }
                        // Advance regardless of PTT success (a persistent hardware fault is surfaced by
                        // the warning above, not by per-tick retry spam), and exclude the just-sent ID
                        // frame from arming the next ID.
                        id_timer.mark_identified(now_ms);
                        tx_frames_seen = engine.frames_transmitted();
                    }
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
/// Drain queued file-transfer fragments to the air as one PTT-keyed burst (assert → transmit all →
/// release), so the half-duplex peer can answer. Called after every command and receive tick; a no-op
/// when the queue is empty. On a PTT-assert failure the burst is dropped and the session's stall/retry
/// path recovers.
/// Upper bound on fragments per keyed burst (the plan §5.3 clamp), independent of the airtime bound.
const MAX_FRAGS_PER_BURST: usize = 64;

/// Split `n` queued fragments into airtime-bounded bursts, returning the fragment count of each burst
/// (which sum to `n`). A burst holds at most `max_frags` fragments and, past its first, stops before
/// its estimated airtime would exceed `burst_max_secs`; the first fragment is always taken, so a lone
/// oversized fragment still forms its own (never empty) burst. `air_secs(i)` estimates fragment `i`.
fn plan_bursts(
    n: usize,
    air_secs: impl Fn(usize) -> f64,
    burst_max_secs: f64,
    max_frags: usize,
) -> Vec<usize> {
    let mut bursts = Vec::new();
    let mut i = 0;
    while i < n {
        let mut count = 0;
        let mut acc = 0.0f64;
        while i + count < n && count < max_frags {
            let secs = air_secs(i + count).max(0.0);
            if count > 0 && acc + secs > burst_max_secs {
                break; // keep the first fragment even if it alone exceeds the budget
            }
            acc += secs;
            count += 1;
        }
        bursts.push(count);
        i += count;
    }
    bursts
}

/// Drain the file-transfer TX queue as one or more **airtime-bounded** PTT-keyed bursts: each burst is
/// its own assert → transmit → release cycle, sized by [`plan_bursts`] so no single keying exceeds
/// `burst_max_secs` (keeps a large transfer under the radio's PTT watchdog and yields between bursts).
fn drain_filexfer_tx(
    engine: &mut ModemEngine,
    ptt_controller: &mut Option<Box<dyn PttController>>,
    event_tx: &std::sync::Arc<tokio::sync::broadcast::Sender<crate::protocol::ControlEvent>>,
    runtime_state: &mut RuntimeControlState,
) {
    use crate::protocol::ControlEvent;
    if runtime_state.filexfer_tx_queue.is_empty() {
        return;
    }
    let queue = std::mem::take(&mut runtime_state.filexfer_tx_queue);
    let burst_max = runtime_state.filexfer_policy.burst_max_secs;

    // Plan bursts up front (immutable engine borrow) so the keying loop can borrow the engine mutably.
    let plan = plan_bursts(
        queue.len(),
        |idx| {
            engine
                .estimate_air_secs(queue[idx].0.len(), &queue[idx].1)
                .unwrap_or(0.0)
        },
        burst_max,
        MAX_FRAGS_PER_BURST,
    );

    let mut idx = 0;
    for count in plan {
        let burst = &queue[idx..idx + count];
        idx += count;
        if let Some(ptt) = ptt_controller.as_mut() {
            if let Err(e) = ptt.assert_ptt() {
                tracing::warn!("filexfer PTT assert failed: {e}");
                return;
            }
        }
        runtime_state.ptt_asserted_at = Some(std::time::Instant::now()); // arm watchdog
        let _ = event_tx.send(ControlEvent::PttChanged { active: true });
        for (frag, mode) in burst {
            let _ = tokio::task::block_in_place(|| engine.transmit(frag, mode, None));
        }
        if let Some(ptt) = ptt_controller.as_mut() {
            if let Err(e) = ptt.release_ptt() {
                tracing::warn!("filexfer PTT release failed: {e}");
            } else {
                runtime_state.ptt_asserted_at = None;
            }
        } else {
            runtime_state.ptt_asserted_at = None;
        }
        let _ = event_tx.send(ControlEvent::PttChanged { active: false });
    }
}

// ── JS8 discovery (FF-15) ────────────────────────────────────────────────────

/// Build the JS8 discovery runtime from `[discovery]` config. Always built (so Enable/Disable work at
/// runtime); the config `enabled` flag gates activation. The initial calling frequency is the 20 m
/// entry (or the first band in the table); `discovery_tick` re-selects the entry for the operator's
/// current home band before each activation. `None` only if the band table is empty.
fn build_discovery_runtime(
    cfg: &openpulse_config::OpenpulseConfig,
) -> Option<openpulse_discovery::DiscoveryRuntime> {
    use openpulse_discovery::{
        DiscoveryParams, DiscoveryRuntime, HintPayload, Submode, TxMode, CAP_HPX, CAP_QSY,
        CAP_RENDEZVOUS,
    };
    let d = &cfg.discovery;
    // Beacon/full opt into TX (Phase E, §97.221 doc in place); anything else is RX-only. TX also
    // requires a callsign — an empty one keeps the station silent regardless of mode.
    let tx_mode = match d.mode.trim().to_ascii_lowercase().as_str() {
        "beacon" => TxMode::Beacon,
        "full" => TxMode::Full,
        _ => TxMode::RxOnly,
    };
    let calling = d
        .calling_freqs_hz
        .get("20m")
        .copied()
        .or_else(|| d.calling_freqs_hz.values().next().copied())?;
    let submode = match d.submode.to_ascii_lowercase().as_str() {
        "slow" => Submode::Slow,
        "fast" => Submode::Fast,
        "turbo" => Submode::Turbo,
        "ultra" => Submode::Ultra,
        _ => Submode::Normal,
    };
    // Advertise what this station can do; pref-channel none (63), NORMAL listen submode.
    let hint = Some(HintPayload {
        caps: CAP_HPX | CAP_RENDEZVOUS | CAP_QSY,
        pref_channel: 63,
        listen_submode: 0,
    });
    Some(DiscoveryRuntime::new(DiscoveryParams {
        enabled: d.enabled,
        idle_grace_ms: d.idle_grace_secs.saturating_mul(1000),
        dwell_ms: d.dwell_secs.saturating_mul(1000),
        station_ttl_ms: d.station_ttl_secs.saturating_mul(1000),
        submode,
        calling_freq_hz: calling,
        tx_mode,
        callsign: cfg.station.callsign.clone(),
        grid: cfg.station.grid_square.clone(),
        hint,
        heartbeat_interval_slots: d.heartbeat_interval_slots.max(1) as u64,
        hint_interval_beacons: d.hint_interval_beacons as u64,
        tx_offset_hz: 1500.0,
        max_clock_skew_ms: d.max_clock_skew_ms,
    }))
}

/// Startup bandplan gate for the rendezvous channel table: log a warning for any configured working
/// frequency the default bandplan flags (out of band / wrong segment). Advisory only — the operator's
/// channels are honoured; this surfaces a likely misconfiguration before it is used on air.
/// Whether the WebSocket control port must be disabled because control auth is required but the WS
/// endpoint cannot authenticate: true if the TCP port needs auth, or the WS bind is itself non-loopback.
fn ws_disabled_for_auth(tcp_require_auth: bool, ws_bind_addr: &str) -> bool {
    tcp_require_auth || openpulse_linksec::auth_required(ws_bind_addr, false)
}

fn validate_rendezvous_channels(cfg: &openpulse_config::OpenpulseConfig) {
    let policy = openpulse_qsy::bandplan::BandplanPolicy::default();
    for (band, freqs) in &cfg.discovery.rendezvous_channels_hz {
        for (idx, &hz) in freqs.iter().enumerate() {
            if let Err(e) = policy.validate_frequency(hz, "DATA") {
                tracing::warn!(
                    band = %band,
                    index = idx,
                    freq_hz = hz,
                    "rendezvous channel fails the bandplan check: {e}"
                );
            }
        }
    }
}

/// Retune the rig to `hz` (no rig / loopback counts as success).
fn discovery_retune(rig: &mut Option<&mut (dyn CatController + Send)>, hz: u64) -> bool {
    match rig.as_mut() {
        Some(c) => c.set_frequency(hz).is_ok(),
        None => true,
    }
}

/// One JS8 NORMAL T/R slot in ms (the discovery MVP is NORMAL-only). Used to convert a rendezvous
/// `switch_in_slots` count into a wall-clock QSY deadline.
const JS8_NORMAL_SLOT_MS: u64 = 15_000;

/// Whether discovery is parked on the JS8 calling channel (Dwelling) — the only state in which the
/// rx-tick tees its raw capture audio to the weak-signal decoder (the DCD-burst pipeline can't carry
/// −24 dB JS8; §6.2). Extracted from the rx-tick `select!` arm so the tee predicate is unit-testable.
fn discovery_is_dwelling(rs: &RuntimeControlState) -> bool {
    rs.discovery
        .as_ref()
        .is_some_and(|d| d.state() == openpulse_discovery::DiscoveryState::Dwelling)
}

/// Consume a completed-rendezvous QSY readiness into the `ConnectPeer` command that hands off to the
/// signed session on the agreed working channel, or `None` when no rendezvous is ready. Extracted from
/// the rx-tick `select!` arm so the peer→command mapping and take-once semantics are unit-testable.
fn take_rendezvous_connect(
    rs: &mut RuntimeControlState,
) -> Option<crate::protocol::ControlCommand> {
    rs.rendezvous_connect_ready
        .take()
        .map(|(peer, _freq_hz)| crate::protocol::ControlCommand::ConnectPeer { callsign: peer })
}

/// Feed one rx-tick's raw audio + the idle predicate into the discovery runtime and execute its
/// outcomes (retune via CAT, home-frequency tracking, event forwarding). No-op when unconfigured.
fn discovery_tick(
    runtime_state: &mut RuntimeControlState,
    engine: &ModemEngine,
    mut rig: Option<&mut (dyn CatController + Send)>,
    event_tx: &std::sync::Arc<tokio::sync::broadcast::Sender<crate::protocol::ControlEvent>>,
    raw_samples: &[f32],
    now_ms: u64,
) -> Option<(Vec<f32>, String)> {
    use openpulse_discovery::DiscoveryOutcome as O;
    runtime_state.discovery.as_ref()?; // nothing to do without a discovery runtime

    // A scheduled post-rendezvous QSY that has come due: both stations retune to the agreed working
    // frequency and hand off to the signed session. The `switch_in_slots` delay ensured the Accept was
    // heard first. We drop the discovery home so the stand-down does not tune back — the QSO owns the
    // dial now — and leave the handoff itself to `server::run` (it holds `&mut engine`).
    if let Some((peer, freq_hz, due_at_ms)) = runtime_state.rendezvous_qsy_due.clone() {
        if now_ms >= due_at_ms {
            runtime_state.rendezvous_qsy_due = None;
            runtime_state.discovery_home_freq_hz = None;
            if discovery_retune(&mut rig, freq_hz) {
                runtime_state.last_freq_hz = Some(freq_hz);
            }
            if let Some(rt) = runtime_state.discovery.as_mut() {
                let _ = rt.preempt(); // stand discovery down; home is cleared so RestoreHome is a no-op
            }
            runtime_state.rendezvous_connect_ready = Some((peer, freq_hz));
            crate::emit_discovery_status(runtime_state, event_tx);
            return None;
        }
    }
    // Simplified idle predicate (plan §4.3): the modem is free of any session/handshake/transfer.
    let idle = engine.hpx_state() == openpulse_core::hpx::HpxState::Idle
        && runtime_state.pending_handshake.is_none()
        && runtime_state.file_rx.is_none()
        && runtime_state.file_tx.is_none()
        && !engine.ota_active();
    // While inactive, target the JS8 calling frequency for the operator's current home band, so
    // activation QSYs within-band instead of always to 20 m. `last_freq_hz` is the home dial while
    // inactive (it becomes the JS8 freq once dwelling, so only refresh before activation).
    let inactive = runtime_state.discovery.as_ref().map(|d| d.state())
        == Some(openpulse_discovery::DiscoveryState::Inactive);
    let home_band = inactive
        .then(|| {
            runtime_state
                .last_freq_hz
                .and_then(openpulse_qsy::bandplan::band_label_for_hz)
        })
        .flatten();
    let per_band_freq =
        home_band.and_then(|label| runtime_state.discovery_calling_freqs_hz.get(label).copied());
    // The responder's usable rendezvous channels are the indices of the home band's working-channel
    // table (empty ⇒ any inbound proposal is rejected `NoCommonFreq`).
    let per_band_channels: Option<Vec<u8>> = home_band.map(|label| {
        let n = runtime_state
            .discovery_rendezvous_channels_hz
            .get(label)
            .map_or(0, |v| v.len().min(u8::MAX as usize));
        (0..n as u8).collect()
    });
    // The working-channel table for the band we are dwelling on, to resolve an agreed channel index→Hz.
    let dwell_channels: Option<Vec<u64>> = runtime_state
        .discovery
        .as_ref()
        .map(|d| d.dial_freq_hz())
        .and_then(openpulse_qsy::bandplan::band_label_for_hz)
        .and_then(|label| {
            runtime_state
                .discovery_rendezvous_channels_hz
                .get(label)
                .cloned()
        });
    // Decode (on slot boundaries) runs inside `tick`; `block_in_place` keeps the async loop responsive.
    let outcomes = tokio::task::block_in_place(|| {
        let rt = runtime_state.discovery.as_mut().expect("checked above");
        if let Some(hz) = per_band_freq {
            rt.set_dial_freq_hz(hz);
        }
        if let Some(ch) = per_band_channels {
            rt.set_rendezvous_channels(ch);
        }
        rt.push_audio(raw_samples);
        rt.tick(now_ms, idle)
    });
    let mut heard_peer = false;
    let mut pending_beacon: Option<(Vec<f32>, String)> = None;
    for o in outcomes {
        match o {
            O::TransmitBeacon { audio, mode } => {
                // Direct DCD gate at the emit decision (not the 0.3-persistence CSMA, which would
                // break slot alignment): defer the beacon if the channel is occupied.
                if engine.is_channel_busy() {
                    tracing::debug!("discovery: deferring beacon — channel busy");
                } else {
                    pending_beacon = Some((audio, mode));
                }
            }
            O::Retune { dial_freq_hz } => {
                runtime_state.discovery_home_freq_hz = runtime_state.last_freq_hz;
                let ok = discovery_retune(&mut rig, dial_freq_hz);
                if ok {
                    runtime_state.last_freq_hz = Some(dial_freq_hz);
                }
                if let Some(rt) = runtime_state.discovery.as_mut() {
                    let _ = rt.qsy_complete(ok);
                }
                crate::emit_discovery_status(runtime_state, event_tx);
            }
            O::RestoreHome => {
                if let Some(home) = runtime_state.discovery_home_freq_hz.take() {
                    if discovery_retune(&mut rig, home) {
                        runtime_state.last_freq_hz = Some(home);
                    }
                }
                crate::emit_discovery_status(runtime_state, event_tx);
            }
            O::StateChanged(_) => crate::emit_discovery_status(runtime_state, event_tx),
            O::StationHeard {
                callsign,
                grid,
                is_new,
            } => {
                heard_peer = true;
                let _ = event_tx.send(crate::protocol::ControlEvent::StationHeard {
                    callsign,
                    grid: grid.unwrap_or_default(),
                    is_new,
                });
            }
            O::RendezvousAgreed {
                peer,
                channel,
                switch_in_slots,
            } => {
                // Resolve the agreed channel index to Hz via the dwelling band's table, surface the
                // agreement, and schedule the QSY + handoff for after the switch delay (so the Accept is
                // heard and both stations retune together).
                match dwell_channels
                    .as_ref()
                    .and_then(|v| v.get(channel as usize))
                    .copied()
                {
                    Some(freq_hz) => {
                        let due_at_ms = now_ms + switch_in_slots as u64 * JS8_NORMAL_SLOT_MS;
                        runtime_state.rendezvous_qsy_due = Some((peer.clone(), freq_hz, due_at_ms));
                        let _ = event_tx.send(crate::protocol::ControlEvent::RendezvousAgreed {
                            peer,
                            freq_hz,
                        });
                    }
                    None => {
                        tracing::warn!(peer = %peer, channel, "rendezvous agreed on an unknown channel index");
                        let _ = event_tx.send(crate::protocol::ControlEvent::RendezvousFailed {
                            peer,
                            reason: "agreed channel index has no configured frequency".to_string(),
                        });
                    }
                }
            }
            O::RendezvousRejected { peer, reason } => {
                let _ = event_tx.send(crate::protocol::ControlEvent::RendezvousFailed {
                    peer,
                    reason: format!("peer declined ({reason:?})"),
                });
            }
            O::RendezvousTimedOut { peer } => {
                let _ = event_tx.send(crate::protocol::ControlEvent::RendezvousFailed {
                    peer,
                    reason: "no reply before timeout".to_string(),
                });
            }
        }
    }
    // Fold any newly-heard OpenPulse-marked stations into the shared peer cache (§5.2).
    if heard_peer {
        crate::sync_discovered_peers(runtime_state, now_ms);
    }
    // Hand a due beacon frame back to the caller, which owns the PTT + `&mut engine` needed to key
    // the transmitter and emit it (see `transmit_beacon_with_ptt`).
    pending_beacon
}

/// UTC epoch milliseconds now.
fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Force-release the transmitter when the PTT watchdog deadline has elapsed. Idempotent: `check_ptt_watchdog`
/// fires once when the deadline passes and clears `ptt_asserted_at`. Called from both the dedicated
/// watchdog `select!` arm (flood-proof) and the rx tick.
fn release_ptt_on_watchdog(
    runtime_state: &mut RuntimeControlState,
    event_tx: &std::sync::Arc<tokio::sync::broadcast::Sender<crate::protocol::ControlEvent>>,
    ptt_controller: &mut Option<Box<dyn PttController>>,
) {
    if check_ptt_watchdog(runtime_state, event_tx) {
        if let Some(ref mut ptt) = ptt_controller {
            if let Err(e) = ptt.release_ptt() {
                tracing::warn!("PTT watchdog release failed: {e}");
            }
        }
    }
}

/// Key PTT, emit one JS8 beacon frame via the raw-audio seam, and release PTT. Any PTT/transmit error
/// is logged and the beacon slot is skipped (never leaves the transmitter keyed).
fn transmit_beacon_with_ptt(
    engine: &mut ModemEngine,
    ptt_controller: &mut Option<Box<dyn PttController>>,
    asserted_at: &mut Option<std::time::Instant>,
    audio: &[f32],
    mode: &str,
) {
    if let Some(ptt) = ptt_controller {
        if let Err(e) = ptt.assert_ptt() {
            tracing::warn!("discovery beacon PTT assert failed: {e}");
            return;
        }
    }
    *asserted_at = Some(std::time::Instant::now()); // arm the watchdog for this keyed burst
    if let Err(e) = engine.transmit_raw_audio(audio, mode, None) {
        tracing::warn!("discovery beacon transmit failed: {e}");
    }
    if let Some(ptt) = ptt_controller {
        if let Err(e) = ptt.release_ptt() {
            tracing::warn!("discovery beacon PTT release failed: {e}");
            return; // leave the watchdog armed so it force-releases the still-keyed transmitter
        }
    }
    *asserted_at = None;
}

/// Apply a manual `PttAssert`/`PttRelease` command to the PTT hardware, synchronously.
///
/// Returns `true` when the hardware call failed (a stuck/absent rig): the caller then skips the
/// engine dispatch so no spurious `PttChanged` tells clients PTT is active when it is not. Any other
/// command (or no controller) is a no-op returning `false`.
fn handle_ptt_command(
    cmd: &crate::Command,
    ptt_controller: &mut Option<Box<dyn PttController>>,
) -> bool {
    let Some(ptt) = ptt_controller.as_mut() else {
        return false;
    };
    let (result, action) = match cmd {
        crate::Command::PttAssert => (ptt.assert_ptt(), "assert"),
        crate::Command::PttRelease => (ptt.release_ptt(), "release"),
        _ => return false,
    };
    if let Err(e) = result {
        tracing::warn!("PTT {action} failed: {e}");
        return true;
    }
    false
}

fn ota_send_with_ptt(
    engine: &mut ModemEngine,
    ptt_controller: &mut Option<Box<dyn PttController>>,
    asserted_at: &mut Option<std::time::Instant>,
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
        *asserted_at = Some(std::time::Instant::now()); // arm the watchdog for this keyed burst
        let _ = event_tx.send(ControlEvent::PttChanged { active: true });
        let tx =
            tokio::task::block_in_place(|| engine.transmit_with_fec_mode(body, &mode, fec, None));
        // Release PTT before listening (half-duplex turnaround).
        if let Some(ptt) = ptt_controller.as_mut() {
            if let Err(e) = ptt.release_ptt() {
                tracing::warn!("OTA send PTT release failed: {e}");
            } else {
                *asserted_at = None;
            }
        } else {
            *asserted_at = None;
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
/// Load the control-channel PSK from `OPENPULSE_CONTROL_PSK` (64 hex chars = 32 bytes).
///
/// This is the initial, testable source; keystore-backed loading (`openpulse-keystore`) is the
/// production follow-up. Returns `Ok(None)` when the variable is unset.
fn load_control_psk() -> Result<Option<[u8; openpulse_linksec::PSK_LEN]>, String> {
    let hex = match std::env::var("OPENPULSE_CONTROL_PSK") {
        Ok(h) => h,
        Err(_) => return Ok(None),
    };
    let hex = hex.trim();
    if hex.len() != openpulse_linksec::PSK_LEN * 2 {
        return Err(format!(
            "OPENPULSE_CONTROL_PSK must be {} hex chars (32 bytes)",
            openpulse_linksec::PSK_LEN * 2
        ));
    }
    let mut out = [0u8; openpulse_linksec::PSK_LEN];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| "OPENPULSE_CONTROL_PSK is not valid hex".to_string())?;
    }
    Ok(Some(out))
}

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

/// CAT controller backend chosen at startup. A concrete enum (not a `Box<dyn>`) so the daemon can
/// reborrow it each loop iteration without the trait-object `Drop`-in-loop borrow-checker snag.
pub enum CatBackend {
    /// hamlib `rigctld` over TCP.
    Rigctld(RigctldController),
    /// TOML-scripted serial CAT (Unix; `generic-serial` feature).
    #[cfg(feature = "generic-serial")]
    Generic(openpulse_radio::GenericSerialCat),
}

impl CatController for CatBackend {
    fn set_frequency(&mut self, hz: u64) -> Result<(), openpulse_radio::RadioError> {
        match self {
            CatBackend::Rigctld(c) => c.set_frequency(hz),
            #[cfg(feature = "generic-serial")]
            CatBackend::Generic(c) => c.set_frequency(hz),
        }
    }
    fn get_frequency(&mut self) -> Result<u64, openpulse_radio::RadioError> {
        match self {
            CatBackend::Rigctld(c) => c.get_frequency(),
            #[cfg(feature = "generic-serial")]
            CatBackend::Generic(c) => c.get_frequency(),
        }
    }
    fn set_mode(
        &mut self,
        mode: &openpulse_radio::RigMode,
    ) -> Result<(), openpulse_radio::RadioError> {
        match self {
            CatBackend::Rigctld(c) => c.set_mode(mode),
            #[cfg(feature = "generic-serial")]
            CatBackend::Generic(c) => c.set_mode(mode),
        }
    }
}

/// Build the CAT (frequency/mode) controller selected by `[radio] cat_backend`:
/// `"none"` → no CAT; `"generic"` → TOML-scripted serial (requires the `generic-serial` feature);
/// anything else → rigctld over TCP. Returns `None` (manual tuning) on a connect/open failure.
pub fn build_cat_controller(radio: &openpulse_config::RadioConfig) -> Option<CatBackend> {
    match radio.cat_backend.to_ascii_lowercase().as_str() {
        "none" => {
            tracing::info!("CAT disabled (cat_backend = \"none\"); manual frequency control");
            None
        }
        "generic" => {
            #[cfg(feature = "generic-serial")]
            {
                match openpulse_radio::GenericSerialCat::open(&radio.serial_port, &radio.rig_file) {
                    Ok(c) => {
                        tracing::info!(port = %radio.serial_port, rig_file = %radio.rig_file,
                            "generic serial CAT backend opened");
                        Some(CatBackend::Generic(c))
                    }
                    Err(err) => {
                        tracing::warn!(port = %radio.serial_port, error = %err,
                            "generic serial CAT open failed; set_freq commands will emit command_error");
                        None
                    }
                }
            }
            #[cfg(not(feature = "generic-serial"))]
            {
                tracing::warn!(
                    "cat_backend = \"generic\" requires the `generic-serial` build feature; \
                     CAT disabled (manual frequency control)"
                );
                None
            }
        }
        _ => match RigctldController::connect(&radio.rigctld_addr) {
            Ok(controller) => Some(CatBackend::Rigctld(controller)),
            Err(err) => {
                tracing::warn!(addr = %radio.rigctld_addr, error = %err,
                    "rigctld connect failed; set_freq commands will emit command_error");
                None
            }
        },
    }
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

#[cfg(test)]
mod burst_planning_tests {
    use super::{plan_bursts, MAX_FRAGS_PER_BURST};

    #[test]
    fn empty_queue_plans_nothing() {
        assert!(plan_bursts(0, |_| 1.0, 20.0, MAX_FRAGS_PER_BURST).is_empty());
    }

    #[test]
    fn small_transfer_fits_one_burst() {
        // 5 fragments × 2 s = 10 s ≤ 20 s budget → a single burst.
        assert_eq!(plan_bursts(5, |_| 2.0, 20.0, MAX_FRAGS_PER_BURST), vec![5]);
    }

    #[test]
    fn airtime_budget_splits_into_multiple_bursts() {
        // Each fragment 6 s, budget 20 s → 3 per burst (18 s), 10 fragments → 3+3+3+1.
        assert_eq!(
            plan_bursts(10, |_| 6.0, 20.0, MAX_FRAGS_PER_BURST),
            vec![3, 3, 3, 1]
        );
    }

    #[test]
    fn oversized_fragment_still_forms_its_own_burst() {
        // A single 50 s fragment exceeds the 20 s budget but must still be sent (never a zero burst).
        assert_eq!(
            plan_bursts(3, |_| 50.0, 20.0, MAX_FRAGS_PER_BURST),
            vec![1, 1, 1]
        );
    }

    #[test]
    fn fragment_count_is_clamped_even_when_airtime_is_tiny() {
        // Negligible airtime would pack everything, but max_frags caps each burst.
        assert_eq!(
            plan_bursts(150, |_| 0.001, 20.0, MAX_FRAGS_PER_BURST),
            vec![64, 64, 22]
        );
    }

    #[test]
    fn per_fragment_airtime_is_respected() {
        // Mixed sizes: 15 s, then 10 s (25 > 20 → new burst), then 3 s (13 ≤ 20 packs with the 10 s).
        let secs = [15.0, 10.0, 3.0];
        assert_eq!(
            plan_bursts(3, |i| secs[i], 20.0, MAX_FRAGS_PER_BURST),
            vec![1, 2]
        );
    }
}

#[cfg(test)]
mod discovery_tick_tests {
    use super::*;
    use crate::protocol::ControlEvent;
    use openpulse_audio::LoopbackBackend;
    use openpulse_discovery::{DiscoveryParams, DiscoveryRuntime, DiscoveryState, Submode, TxMode};
    use openpulse_radio::PttController;

    /// A NORMAL slot of audio with one heartbeat (KN4CRD EM73) at 1500 Hz (C-2 upstream vector).
    fn heartbeat_slot() -> Vec<f32> {
        use js8_plugin::costas::CostasKind;
        use js8_plugin::message::js8_info_bits;
        use js8_plugin::modulate::{modulate_tones, GfskParams};
        use js8_plugin::submode::params;
        use js8_plugin::tones::message_to_tones;
        let payload: [u8; 9] = [0x0a, 0x2f, 0xb3, 0xa3, 0xee, 0x2e, 0xe2, 0xea, 0x58];
        let info = js8_info_bits(&payload, 0);
        let sm = params(js8_plugin::submode::Submode::Normal);
        modulate_tones(
            &message_to_tones(&info, CostasKind::Original),
            1500.0,
            &GfskParams::from_submode(&sm),
        )
    }

    #[test]
    fn discovery_tick_activates_dwells_and_hears_an_injected_station() {
        let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
        let _ = &mut engine; // hpx_state() defaults to Idle → idle predicate holds
        let (tx, mut rx) = tokio::sync::broadcast::channel::<ControlEvent>(64);
        let ev = std::sync::Arc::new(tx);
        let mut rs = RuntimeControlState {
            last_freq_hz: Some(14_074_000), // a home frequency to save/restore
            discovery: Some(DiscoveryRuntime::new(DiscoveryParams {
                enabled: true,
                idle_grace_ms: 0,
                dwell_ms: 0,
                station_ttl_ms: 3_600_000,
                submode: Submode::Normal,
                calling_freq_hz: 14_078_000,
                tx_mode: openpulse_discovery::TxMode::RxOnly,
                callsign: String::new(),
                grid: String::new(),
                hint: None,
                heartbeat_interval_slots: 8,
                hint_interval_beacons: 3,
                tx_offset_hz: 1500.0,
                max_clock_skew_ms: 2000,
            })),
            ..RuntimeControlState::default()
        };

        // Tick 1: idle → activate → (no rig) retune ok → dwell.
        discovery_tick(&mut rs, &engine, None, &ev, &[], 1000);
        assert_eq!(
            rs.discovery.as_ref().unwrap().state(),
            DiscoveryState::Dwelling
        );
        assert_eq!(rs.discovery_home_freq_hz, Some(14_074_000), "home saved");
        assert_eq!(
            rs.last_freq_hz,
            Some(14_078_000),
            "tuned to the JS8 calling freq"
        );

        // Tick 2 (same slot): buffer the heartbeat audio.
        discovery_tick(&mut rs, &engine, None, &ev, &heartbeat_slot(), 1000);
        // Tick 3: next UTC slot → decode → StationHeard.
        discovery_tick(&mut rs, &engine, None, &ev, &[], 16_000);

        // The station is cached and a StationHeard event was emitted.
        assert!(rs
            .discovery
            .as_ref()
            .unwrap()
            .stations()
            .get("KN4CRD")
            .is_some());
        let mut heard = false;
        while let Ok(e) = rx.try_recv() {
            if let ControlEvent::StationHeard { callsign, grid, .. } = e {
                if callsign == "KN4CRD" && grid == "EM73" {
                    heard = true;
                }
            }
        }
        assert!(heard, "a StationHeard event for KN4CRD/EM73 was emitted");
    }

    #[test]
    fn dwelling_predicate_gates_the_dwell_audio_tee() {
        // The rx-tick tees raw capture audio to the weak-signal decoder only while dwelling. Guard the
        // predicate that gates it (deleted/inverted → discovery never sees calling-channel audio, or the
        // DCD pipeline is fed −24 dB JS8 it can't carry). Companion to the inline tee in `server::run`.
        let engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
        let (tx, _rx) = tokio::sync::broadcast::channel::<ControlEvent>(64);
        let ev = std::sync::Arc::new(tx);

        // No discovery runtime → never tee.
        let mut none_rs = RuntimeControlState::default();
        assert!(
            !discovery_is_dwelling(&none_rs),
            "unconfigured never dwells"
        );
        let _ = &mut none_rs;

        let mut rs = RuntimeControlState {
            last_freq_hz: Some(14_074_000),
            discovery: Some(DiscoveryRuntime::new(DiscoveryParams {
                enabled: true,
                idle_grace_ms: 0,
                dwell_ms: 0,
                station_ttl_ms: 3_600_000,
                submode: Submode::Normal,
                calling_freq_hz: 14_078_000,
                tx_mode: TxMode::RxOnly,
                callsign: String::new(),
                grid: String::new(),
                hint: None,
                heartbeat_interval_slots: 8,
                hint_interval_beacons: 3,
                tx_offset_hz: 1500.0,
                max_clock_skew_ms: 2000,
            })),
            ..RuntimeControlState::default()
        };

        // Before the first tick the runtime is Inactive → no tee.
        assert_ne!(
            rs.discovery.as_ref().unwrap().state(),
            DiscoveryState::Dwelling
        );
        assert!(
            !discovery_is_dwelling(&rs),
            "an inactive runtime does not tee audio"
        );

        // One tick activates → dwells on the calling freq → the tee opens.
        discovery_tick(&mut rs, &engine, None, &ev, &[], 1000);
        assert_eq!(
            rs.discovery.as_ref().unwrap().state(),
            DiscoveryState::Dwelling
        );
        assert!(
            discovery_is_dwelling(&rs),
            "a dwelling runtime tees the tick's raw audio"
        );
    }

    #[test]
    fn take_rendezvous_connect_maps_ready_peer_and_consumes_it() {
        // The completed-rendezvous QSY hands off to the signed session by mapping the ready (peer, freq)
        // into a `ConnectPeer` for that peer, consumed once. Guard the mapping + take-once semantics of
        // the inline `server::run` handoff (which then feeds the command to `apply_command_to_engine`).
        use crate::protocol::ControlCommand;

        let mut rs = RuntimeControlState::default();
        assert!(
            take_rendezvous_connect(&mut rs).is_none(),
            "no ready rendezvous → no connect"
        );

        rs.rendezvous_connect_ready = Some(("W1AW".into(), 14_101_000));
        match take_rendezvous_connect(&mut rs) {
            Some(ControlCommand::ConnectPeer { callsign }) => assert_eq!(callsign, "W1AW"),
            other => panic!("expected ConnectPeer for W1AW, got {other:?}"),
        }
        assert!(
            rs.rendezvous_connect_ready.is_none(),
            "the readiness is consumed (take-once) so the handoff fires exactly once"
        );
        assert!(
            take_rendezvous_connect(&mut rs).is_none(),
            "a second poll after consumption yields nothing"
        );
    }

    /// One NORMAL beacon frame (payload9 + its i3bit flag) modulated at 1500 Hz.
    fn beacon_frame(hex: &str, i3bit: u8) -> Vec<f32> {
        use js8_plugin::costas::CostasKind;
        use js8_plugin::message::js8_info_bits;
        use js8_plugin::modulate::{modulate_tones, GfskParams};
        use js8_plugin::submode::params;
        use js8_plugin::tones::message_to_tones;
        let mut p = [0u8; 9];
        for (i, b) in p.iter_mut().enumerate() {
            *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
        }
        let info = js8_info_bits(&p, i3bit);
        let sm = params(js8_plugin::submode::Submode::Normal);
        modulate_tones(
            &message_to_tones(&info, CostasKind::Original),
            1500.0,
            &GfskParams::from_submode(&sm),
        )
    }

    #[test]
    fn discovery_tick_transmits_a_beacon_in_beacon_mode() {
        let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
        let (tx, _rx) = tokio::sync::broadcast::channel::<ControlEvent>(64);
        let ev = std::sync::Arc::new(tx);
        let mut rs = RuntimeControlState {
            last_freq_hz: Some(14_074_000),
            discovery: Some(DiscoveryRuntime::new(DiscoveryParams {
                enabled: true,
                idle_grace_ms: 0,
                dwell_ms: 0,
                station_ttl_ms: 3_600_000,
                submode: Submode::Normal,
                calling_freq_hz: 14_078_000,
                tx_mode: TxMode::Beacon,
                callsign: "DC0SK".into(),
                grid: "JN58".into(),
                hint: None,
                heartbeat_interval_slots: 2,
                hint_interval_beacons: 0,
                tx_offset_hz: 1500.0,
                max_clock_skew_ms: 2000,
            })),
            ..RuntimeControlState::default()
        };

        discovery_tick(&mut rs, &engine, None, &ev, &[], 1000); // activate → dwell
        let mut beacon = None;
        let mut t = 1000u64;
        for _ in 0..4 {
            t += 15_000;
            if let Some(b) = discovery_tick(&mut rs, &engine, None, &ev, &[], t) {
                beacon = Some(b);
            }
        }
        let (audio, mode) = beacon.expect("a heartbeat beacon is due in beacon mode");
        assert_eq!(mode, "JS8-NORMAL");
        assert!(!audio.is_empty());

        // The daemon transmits it via the raw-audio seam (no PTT hardware in the test).
        let mut ptt: Option<Box<dyn PttController>> = None;
        let mut asserted_at = None;
        transmit_beacon_with_ptt(&mut engine, &mut ptt, &mut asserted_at, &audio, &mode);
        assert_eq!(engine.raw_audio_frames_transmitted(), 1);
    }

    #[test]
    fn discovery_tick_defers_a_due_beacon_when_the_channel_is_busy() {
        // The DCD gate at the beacon-emit decision must hold the beacon when the calling channel is
        // occupied (don't key over an in-progress QSO). Same beacon-mode setup as the emit test, but
        // with the engine's DCD driven busy first — no beacon frame may be handed back.
        let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
        engine
            .register_plugin(Box::new(BpskPlugin::new()))
            .expect("register BPSK plugin");

        // Trip DCD: loopback echoes the TX into the RX capture, so the received energy marks the
        // channel busy. Nothing in `discovery_tick` feeds the DCD, so the busy state persists.
        engine
            .transmit(b"occupying signal", "BPSK250", None)
            .unwrap();
        let _ = engine.receive("BPSK250", None).unwrap();
        assert!(
            engine.is_channel_busy(),
            "precondition: channel must read busy"
        );

        let (tx, _rx) = tokio::sync::broadcast::channel::<ControlEvent>(64);
        let ev = std::sync::Arc::new(tx);
        let mut rs = RuntimeControlState {
            last_freq_hz: Some(14_074_000),
            discovery: Some(DiscoveryRuntime::new(DiscoveryParams {
                enabled: true,
                idle_grace_ms: 0,
                dwell_ms: 0,
                station_ttl_ms: 3_600_000,
                submode: Submode::Normal,
                calling_freq_hz: 14_078_000,
                tx_mode: TxMode::Beacon,
                callsign: "DC0SK".into(),
                grid: "JN58".into(),
                hint: None,
                heartbeat_interval_slots: 2,
                hint_interval_beacons: 0,
                tx_offset_hz: 1500.0,
                max_clock_skew_ms: 2000,
            })),
            ..RuntimeControlState::default()
        };

        discovery_tick(&mut rs, &engine, None, &ev, &[], 1000); // activate → dwell
        let mut t = 1000u64;
        for _ in 0..4 {
            t += 15_000;
            assert!(
                discovery_tick(&mut rs, &engine, None, &ev, &[], t).is_none(),
                "a busy channel must defer the beacon — none may be transmitted"
            );
        }
    }

    /// A PTT double whose assert and/or release can be made to fail, standing in for a transient
    /// rigctld/serial fault.
    #[derive(Default)]
    struct FlakyPtt {
        fail_assert: bool,
        fail_release: bool,
    }
    impl PttController for FlakyPtt {
        fn assert_ptt(&mut self) -> Result<(), openpulse_radio::PttError> {
            if self.fail_assert {
                Err(openpulse_radio::PttError::Serial("assert failed".into()))
            } else {
                Ok(())
            }
        }
        fn release_ptt(&mut self) -> Result<(), openpulse_radio::PttError> {
            if self.fail_release {
                Err(openpulse_radio::PttError::Serial("stuck keyed".into()))
            } else {
                Ok(())
            }
        }
        fn is_asserted(&self) -> bool {
            false
        }
    }

    #[test]
    fn ptt_command_guard_reports_hardware_failure_to_skip_dispatch() {
        use crate::Command;

        // A failed assert/release must report `true` so the caller skips the engine dispatch and does
        // not emit a spurious PttChanged claiming a state the hardware never reached.
        let mut failing: Option<Box<dyn PttController>> = Some(Box::new(FlakyPtt {
            fail_assert: true,
            fail_release: true,
        }));
        assert!(
            handle_ptt_command(&Command::PttAssert, &mut failing),
            "a failed assert must report hard failure"
        );
        assert!(
            handle_ptt_command(&Command::PttRelease, &mut failing),
            "a failed release must report hard failure"
        );

        // A successful call reports `false` so the dispatch proceeds and the PttChanged fires.
        let mut ok: Option<Box<dyn PttController>> = Some(Box::new(FlakyPtt::default()));
        assert!(!handle_ptt_command(&Command::PttAssert, &mut ok));
        assert!(!handle_ptt_command(&Command::PttRelease, &mut ok));

        // Non-PTT commands and the no-controller case are always no-op passes.
        assert!(!handle_ptt_command(&Command::PttAssert, &mut None));
        assert!(!handle_ptt_command(
            &Command::GetConfig,
            &mut Some(Box::new(FlakyPtt {
                fail_assert: true,
                fail_release: true,
            }))
        ));
    }

    #[test]
    fn automatic_tx_arms_the_watchdog_and_disarms_only_on_clean_release() {
        // Audit #5: an automatic keying path must arm the PTT watchdog so a failed release (rig fault)
        // is caught, and disarm it on a clean release so the watchdog never fires spuriously.
        let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
        let audio = vec![0.0f32; 100];

        // Release fails → the transmitter may still be keyed, so the watchdog must stay armed.
        let mut ptt: Option<Box<dyn PttController>> = Some(Box::new(FlakyPtt {
            fail_release: true,
            ..Default::default()
        }));
        let mut armed = None;
        transmit_beacon_with_ptt(&mut engine, &mut ptt, &mut armed, &audio, "JS8-NORMAL");
        assert!(
            armed.is_some(),
            "a failed release must leave the watchdog armed"
        );

        // Clean release → disarmed, so the watchdog can't fire on a stale timestamp.
        let mut ptt2: Option<Box<dyn PttController>> = Some(Box::new(FlakyPtt::default()));
        let mut armed2 = Some(std::time::Instant::now());
        transmit_beacon_with_ptt(&mut engine, &mut ptt2, &mut armed2, &audio, "JS8-NORMAL");
        assert!(armed2.is_none(), "a clean release disarms the watchdog");
    }

    #[test]
    fn watchdog_releases_the_transmitter_when_the_deadline_passes() {
        // The decoupled watchdog `select!` arm calls this on its own fast timer, so a client command
        // flood can no longer starve the force-release. Verify it releases + disarms + notifies once.
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct CountingPtt(std::sync::Arc<AtomicUsize>);
        impl PttController for CountingPtt {
            fn assert_ptt(&mut self) -> Result<(), openpulse_radio::PttError> {
                Ok(())
            }
            fn release_ptt(&mut self) -> Result<(), openpulse_radio::PttError> {
                self.0.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            fn is_asserted(&self) -> bool {
                false
            }
        }

        let releases = std::sync::Arc::new(AtomicUsize::new(0));
        let mut ptt: Option<Box<dyn PttController>> = Some(Box::new(CountingPtt(releases.clone())));
        let (tx, mut rx) = tokio::sync::broadcast::channel::<ControlEvent>(8);
        let ev = std::sync::Arc::new(tx);

        // Armed, with a deadline already in the past.
        let mut rs = RuntimeControlState {
            ptt_asserted_at: Some(std::time::Instant::now()),
            ptt_max_duration: std::time::Duration::from_nanos(1),
            ..RuntimeControlState::default()
        };

        release_ptt_on_watchdog(&mut rs, &ev, &mut ptt);
        assert_eq!(
            releases.load(Ordering::Relaxed),
            1,
            "the watchdog must force-release the keyed transmitter"
        );
        assert!(
            rs.ptt_asserted_at.is_none(),
            "the watchdog disarms after firing"
        );
        assert!(
            matches!(
                rx.try_recv(),
                Ok(ControlEvent::PttChanged { active: false })
            ),
            "clients are notified the transmitter was released"
        );

        // Idempotent: nothing armed → no second release.
        release_ptt_on_watchdog(&mut rs, &ev, &mut ptt);
        assert_eq!(
            releases.load(Ordering::Relaxed),
            1,
            "no spurious release once disarmed"
        );
    }

    #[test]
    fn discovery_tick_recognizes_an_opulse_peer_into_the_shared_cache() {
        // The four Huffman-forced frames of `DC0SK: @OPULSE OPHF1 1FAX3AIT` (Qt5 ground truth).
        let frames = [
            ("2694fa766ea662ea58", 1u8),
            ("531a90d5639ea3f5c8", 0u8),
            ("bfec6491489275029b", 0u8),
            ("b9afffffffffffffff", 2u8),
        ];
        let engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
        let (tx, mut rx) = tokio::sync::broadcast::channel::<ControlEvent>(64);
        let ev = std::sync::Arc::new(tx);
        let mut rs = RuntimeControlState {
            last_freq_hz: Some(14_074_000),
            discovery: Some(DiscoveryRuntime::new(DiscoveryParams {
                enabled: true,
                idle_grace_ms: 0,
                dwell_ms: 0,
                station_ttl_ms: 3_600_000,
                submode: Submode::Normal,
                calling_freq_hz: 14_078_000,
                tx_mode: openpulse_discovery::TxMode::RxOnly,
                callsign: String::new(),
                grid: String::new(),
                hint: None,
                heartbeat_interval_slots: 8,
                hint_interval_beacons: 3,
                tx_offset_hz: 1500.0,
                max_clock_skew_ms: 2000,
            })),
            ..RuntimeControlState::default()
        };

        discovery_tick(&mut rs, &engine, None, &ev, &[], 1000); // activate → dwell
        let mut t = 1000u64;
        for (hex, i3) in frames {
            discovery_tick(&mut rs, &engine, None, &ev, &beacon_frame(hex, i3), t);
            t += 15_000;
            discovery_tick(&mut rs, &engine, None, &ev, &[], t); // cross boundary → decode
        }

        // The recognized peer is in the shared cache with its capabilities.
        let peers = rs
            .peer_cache
            .query(0, 0, openpulse_core::peer_cache::TrustFilter::Any, 16, t);
        let peer = peers
            .iter()
            .find(|p| p.peer_id == "js8:DC0SK")
            .expect("DC0SK cached as an OpenPulse peer");
        assert_eq!(peer.capability_mask, 0xB105);

        // ListPeers surfaces it.
        crate::emit_peer_list(&mut rs, &ev, t);
        let mut listed = false;
        while let Ok(e) = rx.try_recv() {
            if let ControlEvent::PeerList { peers } = e {
                listed = peers.iter().any(|p| p.peer_id == "js8:DC0SK");
            }
        }
        assert!(listed, "ListPeers reported the recognized peer");
    }

    #[test]
    fn discovery_tick_qsys_to_the_current_home_bands_calling_freq() {
        // Home on 40 m; the runtime's initial calling freq is the 20 m default. Activation must
        // re-select the 40 m entry from the band table and QSY there, not to 20 m.
        let engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
        let (tx, _rx) = tokio::sync::broadcast::channel::<ControlEvent>(64);
        let ev = std::sync::Arc::new(tx);
        let calling: std::collections::BTreeMap<String, u64> =
            [("40m", 7_078_000u64), ("20m", 14_078_000)]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect();
        let mut rs = RuntimeControlState {
            last_freq_hz: Some(7_074_000), // home on 40 m
            discovery_calling_freqs_hz: calling,
            discovery: Some(DiscoveryRuntime::new(DiscoveryParams {
                enabled: true,
                idle_grace_ms: 0,
                dwell_ms: 0,
                station_ttl_ms: 3_600_000,
                submode: Submode::Normal,
                calling_freq_hz: 14_078_000, // 20 m default, must be overridden
                tx_mode: openpulse_discovery::TxMode::RxOnly,
                callsign: String::new(),
                grid: String::new(),
                hint: None,
                heartbeat_interval_slots: 8,
                hint_interval_beacons: 3,
                tx_offset_hz: 1500.0,
                max_clock_skew_ms: 2000,
            })),
            ..RuntimeControlState::default()
        };

        discovery_tick(&mut rs, &engine, None, &ev, &[], 1000);
        assert_eq!(
            rs.discovery.as_ref().unwrap().state(),
            DiscoveryState::Dwelling
        );
        assert_eq!(
            rs.discovery_home_freq_hz,
            Some(7_074_000),
            "40 m home saved"
        );
        assert_eq!(
            rs.last_freq_hz,
            Some(7_078_000),
            "tuned to the 40 m JS8 calling freq, not the 20 m default"
        );
        assert_eq!(
            rs.discovery.as_ref().unwrap().dial_freq_hz(),
            7_078_000,
            "runtime dial freq reflects the per-band selection (drives DiscoveryStatus)"
        );
    }

    #[test]
    fn discovery_tick_is_a_noop_when_unconfigured() {
        let engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
        let (tx, mut rx) = tokio::sync::broadcast::channel::<ControlEvent>(8);
        let ev = std::sync::Arc::new(tx);
        let mut rs = RuntimeControlState::default(); // discovery: None
        discovery_tick(&mut rs, &engine, None, &ev, &[0.0; 1000], 1000);
        assert!(rx.try_recv().is_err(), "no events without discovery");
    }

    /// Audio for one frame of a directed over at 1500 Hz.
    fn directed_frame_audio(f: &js8_plugin::BeaconFrame) -> Vec<f32> {
        js8_plugin::beacon::frame_audio(f, 1500.0, js8_plugin::submode::Submode::Normal)
    }

    #[test]
    fn discovery_tick_responds_to_a_proposal_and_emits_rendezvous_agreed() {
        let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
        let _ = &mut engine;
        let (tx, mut rx) = tokio::sync::broadcast::channel::<ControlEvent>(64);
        let ev = std::sync::Arc::new(tx);
        let mut rs = RuntimeControlState {
            last_freq_hz: Some(14_074_000), // 20 m home
            discovery_rendezvous_channels_hz: [(
                "20m".to_string(),
                vec![14_101_000, 14_103_000, 14_105_000],
            )]
            .into_iter()
            .collect(),
            discovery: Some(DiscoveryRuntime::new(DiscoveryParams {
                enabled: true,
                idle_grace_ms: 0,
                dwell_ms: 0,
                station_ttl_ms: 3_600_000,
                submode: Submode::Normal,
                calling_freq_hz: 14_078_000,
                tx_mode: TxMode::Full, // responder role on
                callsign: "DC0SK".into(),
                grid: "JN58".into(),
                hint: None,
                heartbeat_interval_slots: 10_000, // never beacon during the test
                hint_interval_beacons: 0,
                tx_offset_hz: 1500.0,
                max_clock_skew_ms: 2000,
            })),
            ..RuntimeControlState::default()
        };

        // Activate → dwell (sets the responder's available channels from the 20 m table).
        discovery_tick(&mut rs, &engine, None, &ev, &[], 1000);
        assert_eq!(
            rs.discovery.as_ref().unwrap().state(),
            DiscoveryState::Dwelling
        );

        // KN4CRD proposes channels 1 then 0; both are available → agree on index 1 = 14_103_000.
        let frames = js8_plugin::directed("KN4CRD", "JN58", "DC0SK", "OPHF QSY? R7 C1 C0");
        let mut t = 1000u64;
        for f in &frames {
            discovery_tick(&mut rs, &engine, None, &ev, &directed_frame_audio(f), t);
            t += 15_000;
            discovery_tick(&mut rs, &engine, None, &ev, &[], t);
        }

        // The responder withholds its agreement until the Accept over has fully transmitted (audit #4b),
        // so tick through the Accept frames until RendezvousAgreed surfaces.
        let mut agreed = None;
        for _ in 0..10 {
            t += 15_000;
            discovery_tick(&mut rs, &engine, None, &ev, &[], t);
            while let Ok(e) = rx.try_recv() {
                if let ControlEvent::RendezvousAgreed { peer, freq_hz } = e {
                    agreed = Some((peer, freq_hz));
                }
            }
            if agreed.is_some() {
                break;
            }
        }
        assert_eq!(agreed, Some(("KN4CRD".to_string(), 14_103_000)));

        // The QSY was scheduled (not fired yet); after the switch delay it retunes + arms the handoff.
        assert!(
            rs.rendezvous_qsy_due.is_some(),
            "QSY scheduled after agreement"
        );
        discovery_tick(&mut rs, &engine, None, &ev, &[], t + 10 * 15_000);
        assert_eq!(
            rs.last_freq_hz,
            Some(14_103_000),
            "retuned to the agreed working frequency"
        );
        assert_eq!(
            rs.rendezvous_connect_ready,
            Some(("KN4CRD".to_string(), 14_103_000)),
            "handoff armed for server::run"
        );
        assert!(rs.rendezvous_qsy_due.is_none(), "schedule consumed");
        assert_eq!(
            rs.discovery.as_ref().unwrap().state(),
            DiscoveryState::Inactive,
            "discovery stood down for the QSO"
        );
    }
}

#[cfg(test)]
mod cat_backend_tests {
    use super::build_cat_controller;
    use openpulse_config::RadioConfig;

    #[test]
    fn cat_backend_none_yields_no_controller() {
        let radio = RadioConfig {
            cat_backend: "none".into(),
            ..RadioConfig::default()
        };
        assert!(build_cat_controller(&radio).is_none());
    }

    #[test]
    fn cat_backend_generic_without_a_rig_file_yields_no_controller() {
        // With the feature off this warns and returns None; with it on, opening an empty
        // serial_port/rig_file fails and also returns None. Either way: no controller, no panic.
        let radio = RadioConfig {
            cat_backend: "generic".into(),
            serial_port: String::new(),
            rig_file: String::new(),
            ..RadioConfig::default()
        };
        assert!(build_cat_controller(&radio).is_none());
    }
}

#[cfg(test)]
mod ws_auth_gate_tests {
    use super::ws_disabled_for_auth;

    #[test]
    fn ws_disabled_when_tcp_requires_auth() {
        // TCP auth on (non-loopback TCP bind or require_auth) → WS must be disabled even if WS is loopback.
        assert!(ws_disabled_for_auth(true, "127.0.0.1"));
        assert!(ws_disabled_for_auth(true, "0.0.0.0"));
    }

    #[test]
    fn ws_disabled_when_ws_bind_is_non_loopback() {
        // Even if the TCP port is unauthenticated loopback, a publicly-bound WS port is a bypass → disable.
        assert!(ws_disabled_for_auth(false, "0.0.0.0"));
        assert!(ws_disabled_for_auth(false, "192.168.1.10"));
    }

    #[test]
    fn ws_enabled_only_when_both_are_loopback_and_no_auth() {
        // The one safe case: no TCP auth required and the WS port is loopback-only.
        assert!(!ws_disabled_for_auth(false, "127.0.0.1"));
        assert!(!ws_disabled_for_auth(false, "localhost"));
    }
}
