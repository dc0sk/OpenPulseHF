//! `openpulse-server` binary — wraps the modem engine and exposes the NDJSON
//! control port on TCP port 9000 and WebSocket port 9001 (defaults).

use openpulse_audio::LoopbackBackend;
use openpulse_daemon::{
    apply_command_to_engine, check_ptt_watchdog, process_received_bytes, ws, ControlServer,
    RuntimeControlState,
};
use openpulse_modem::ModemEngine;
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

    let audio = Box::new(LoopbackBackend::default());
    let mut engine = ModemEngine::new(audio);
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

    // Pre-build the cross-band repeater so it is ready when EnableRepeater fires.
    let repeater = {
        let mut rx = ModemEngine::new(Box::new(LoopbackBackend::default()));
        rx.register_plugin(Box::new(BpskPlugin::new())).ok();
        rx.register_plugin(Box::new(QpskPlugin::new())).ok();
        rx.register_plugin(Box::new(Psk8Plugin::new())).ok();
        let mut tx = ModemEngine::new(Box::new(LoopbackBackend::default()));
        tx.register_plugin(Box::new(BpskPlugin::new())).ok();
        tx.register_plugin(Box::new(QpskPlugin::new())).ok();
        tx.register_plugin(Box::new(Psk8Plugin::new())).ok();
        let rep_cfg = RepeaterConfig {
            enabled: true,
            mode: cfg.modem.mode.clone(),
            tx_hang_ms: 0,
            full_duplex: false,
        };
        CrossBandRepeater::new(Box::new(NoOpPtt::new()), rx, tx, rep_cfg)
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
    let mut runtime_state = RuntimeControlState {
        repeater_enabled: cfg.repeater.enabled,
        repeater: Some(repeater),
        qsy_candidate_freqs: cfg.qsy.candidate_freqs_hz.clone(),
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
            }
        }
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
