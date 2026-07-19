use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::broadcast::error::RecvError;

use openpulse_core::trust::{CertificateSource, PublicKeyTrustLevel, SigningMode};
use openpulse_modem::engine::SecureSessionParams;

use crate::bridge::ModemBridge;
use crate::error::ArdopError;
use crate::state::TncState;

/// Maximum command line length accepted before dropping the connection.
const MAX_CMD_LINE: usize = 4096;

/// Read one `\n`-terminated line, but never buffer more than `cap` bytes — `read_line` alone grows its
/// destination without limit, so a client that never sends a newline could otherwise exhaust memory
/// (audit A-1). A line at/over the cap yields `cap` bytes with no trailing newline (the `Take` EOFs),
/// which the caller's `n > MAX_CMD_LINE` check rejects.
async fn read_capped_line<R: tokio::io::AsyncBufRead + Unpin>(
    reader: R,
    line: &mut String,
    cap: u64,
) -> std::io::Result<usize> {
    let mut limited = reader.take(cap);
    limited.read_line(line).await
}

pub async fn serve(listener: TcpListener, bridge: Arc<ModemBridge>) -> Result<(), ArdopError> {
    loop {
        let (stream, addr) = listener.accept().await?;
        tracing::info!("command client connected: {addr}");
        let b = bridge.clone();
        tokio::spawn(async move {
            let result = handle_client(stream, b.clone()).await;
            if let Err(e) = result {
                tracing::warn!("command client {addr} disconnected: {e}");
            }
            // A host that keys the transmitter and then vanishes — crash, kill, dropped network —
            // would otherwise leave the rig transmitting until a human notices: a §97 violation and
            // a PA-damage risk. Release unconditionally on every exit path from `handle_client`
            // (EOF, oversized line, I/O error), not just the clean ones.
            //
            // This releases whenever PTT is down-stream asserted, without tracking which connection
            // keyed it. ARDOP assumes a single controlling host, and an unnecessary unkey is
            // strictly less harmful than a stuck carrier — but that is the tradeoff being made here.
            release_ptt_on_disconnect(&b, addr).await;
        });
    }
}

/// Drop the transmitter if this connection ended while it was still keyed.
///
/// No-op when PTT is already down, so a clean `PTT FALSE` is not followed by a redundant release
/// and a client that never keyed never touches the hardware.
async fn release_ptt_on_disconnect(bridge: &Arc<ModemBridge>, addr: std::net::SocketAddr) {
    let ptt = bridge.ptt.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        let mut guard = ptt.lock().unwrap_or_else(|e| e.into_inner());
        if !guard.is_asserted() {
            return None;
        }
        Some(guard.release_ptt())
    })
    .await;

    match outcome {
        Ok(None) => {}
        Ok(Some(Ok(()))) => {
            tracing::warn!("client {addr} disconnected while keyed — PTT released by the TNC")
        }
        Ok(Some(Err(e))) => {
            tracing::error!(error = %e, "client {addr} disconnected while keyed and PTT release FAILED — transmitter may be stuck")
        }
        Err(_) => {
            tracing::error!("client {addr} disconnected while keyed and the PTT release task panicked — transmitter may be stuck")
        }
    }
}

async fn handle_client(
    stream: tokio::net::TcpStream,
    bridge: Arc<ModemBridge>,
) -> Result<(), ArdopError> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut event_rx = bridge.event_tx.subscribe();
    let mut line = String::new();

    loop {
        line.clear();
        tokio::select! {
            // Bound the read to `MAX_CMD_LINE + 1` bytes so a client that never sends a newline can't
            // grow `line` without limit — `read_line` itself has no cap, so the guard below alone would
            // apply too late (after the oversized buffer was already allocated). A `Take` that EOFs at
            // the cap yields an over-limit line with no trailing '\n', which the `n > MAX_CMD_LINE` /
            // missing-newline check then rejects.
            n = read_capped_line(&mut reader, &mut line, MAX_CMD_LINE as u64 + 1) => {
                let n = n?;
                if n == 0 {
                    break;
                }
                if n > MAX_CMD_LINE {
                    tracing::warn!("command line too long ({n} B), dropping connection");
                    return Err(ArdopError::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "command line too long",
                    )));
                }
                let cmd = line.trim();
                if cmd.is_empty() {
                    continue;
                }
                tracing::debug!("ardop cmd: {cmd}");
                for resp in dispatch(cmd, &bridge).await {
                    write_half.write_all(resp.as_bytes()).await?;
                    write_half.write_all(b"\r\n").await?;
                }
                write_half.flush().await?;
            }
            result = event_rx.recv() => {
                match result {
                    Ok(event) => {
                        write_half.write_all(event.as_bytes()).await?;
                        write_half.write_all(b"\r\n").await?;
                        write_half.flush().await?;
                    }
                    Err(RecvError::Lagged(n)) => {
                        // Slow client; TNC event lines were dropped from the broadcast ring. The old
                        // `Ok(event) =` pattern silently disabled this branch instead — the same bug
                        // already fixed in data.rs and the bridge worker, never swept here. What gets
                        // dropped matters: DISCONNECTED and the §97 `FAULT no MYID` line both travel
                        // this channel (audit 2026-07-19, #12).
                        tracing::warn!(
                            "ARDOP command event stream lagged, {n} TNC event line(s) dropped for \
                             this client"
                        );
                    }
                    Err(RecvError::Closed) => return Ok(()),
                }
            }
        }
    }
    Ok(())
}

async fn dispatch(cmd: &str, bridge: &ModemBridge) -> Vec<String> {
    let parts: Vec<&str> = cmd.splitn(3, ' ').collect();
    let verb = parts[0].to_uppercase();

    match verb.as_str() {
        "VERSION" => vec!["VERSION 1.0-OpenPulseHF".into()],

        "MYID" => {
            if let Some(call) = parts.get(1).filter(|s| !s.is_empty()) {
                *bridge.callsign.write().await = call.to_string();
                // Mirror the operating call into the engine so the §97 regulatory TX-metadata log
                // stamps it (off the executor per the CONNECT/DISCONNECT rationale: the worker can
                // hold the engine mutex across an RF burst).
                let engine = bridge.engine.clone();
                let call_for_engine = call.to_string();
                let _ = tokio::task::spawn_blocking(move || {
                    engine
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .set_callsign(call_for_engine);
                })
                .await;
                vec![format!("MYID {call}")]
            } else {
                vec![format!("MYID {}", bridge.callsign.read().await)]
            }
        }

        "LISTEN" => match parts.get(1).map(|s| s.to_uppercase()).as_deref() {
            Some("TRUE") => {
                bridge.set_state(TncState::Listen).await;
                vec!["LISTEN TRUE".into()]
            }
            _ => {
                let state = bridge.state.read().await.clone();
                if matches!(state, TncState::Listen) {
                    bridge.set_state(TncState::Disc).await;
                }
                vec!["LISTEN FALSE".into()]
            }
        },

        "CONNECT" => {
            // <bw> is parts[1], <call> is parts[2].  Accept missing bw for tolerance.
            let peer = parts
                .get(2)
                .copied()
                .or_else(|| parts.get(1).copied())
                .unwrap_or("UNKNOWN")
                .to_string();
            bridge
                .set_state(TncState::Connecting { peer: peer.clone() })
                .await;
            bridge
                .set_state(TncState::Connected { peer: peer.clone() })
                .await;
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            // Run the blocking engine lock + handshake off the async executor (spawn_blocking): the
            // modem worker holds this same std Mutex across a full RF TX/RX burst, and locking it
            // inline here would park an executor thread for the burst — stalling other clients' commands
            // (including ABORT). Mirrors the PTT handlers below.
            let engine = bridge.engine.clone();
            let peer_for_log = peer.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let mut engine = engine.lock().unwrap_or_else(|e| e.into_inner());
                if let Err(e) = engine.begin_secure_session(
                    SecureSessionParams {
                        local_minimum_mode: SigningMode::Normal,
                        peer_supported_modes: vec![SigningMode::Normal],
                        key_trust: PublicKeyTrustLevel::Unknown,
                        certificate_source: CertificateSource::OverAir,
                        psk_validated: false,
                    },
                    now_ms,
                ) {
                    tracing::warn!(peer = %peer_for_log, error = %e, "begin_secure_session failed on CONNECT");
                }
            })
            .await;
            // Both state transitions returned as direct responses in order.
            vec!["NEWSTATE CONNECTING".into(), format!("CONNECTED {peer}")]
        }

        "DISCONNECT" => {
            bridge.set_state(TncState::Disconnecting).await;
            bridge.set_state(TncState::Disc).await;
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            // Off-executor for the same reason as CONNECT: the worker can hold the engine mutex across
            // an RF burst, so lock it on the blocking pool rather than parking an executor thread.
            let engine = bridge.engine.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let mut engine = engine.lock().unwrap_or_else(|e| e.into_inner());
                if let Err(e) = engine.end_secure_session(now_ms) {
                    tracing::warn!(error = %e, "end_secure_session failed on DISCONNECT");
                }
            })
            .await;
            vec!["NEWSTATE DISCONNECTING".into(), "DISCONNECTED".into()]
        }

        "ABORT" => {
            bridge.set_state(TncState::Disc).await;
            vec!["NEWSTATE DISC".into()]
        }

        "STATE" => {
            vec![format!("STATE {}", bridge.state.read().await.label())]
        }

        "BUFFER" => {
            let n = bridge.tx_pending.load(std::sync::atomic::Ordering::Relaxed);
            vec![format!("BUFFER {n}")]
        }

        "PTT" => match parts.get(1).map(|s| s.to_uppercase()).as_deref() {
            Some("TRUE") => {
                let ptt = bridge.ptt.clone();
                match tokio::task::spawn_blocking(move || {
                    ptt.lock().unwrap_or_else(|e| e.into_inner()).assert_ptt()
                })
                .await
                {
                    Ok(Ok(())) => vec!["PTT TRUE".into()],
                    Ok(Err(e)) => {
                        tracing::error!(error = %e, "PTT assert failed");
                        vec![format!("FAULT PTT assert failed: {e}")]
                    }
                    Err(_) => vec!["FAULT PTT task panicked".into()],
                }
            }
            _ => {
                let ptt = bridge.ptt.clone();
                match tokio::task::spawn_blocking(move || {
                    ptt.lock().unwrap_or_else(|e| e.into_inner()).release_ptt()
                })
                .await
                {
                    Ok(Ok(())) => vec!["PTT FALSE".into()],
                    Ok(Err(e)) => {
                        tracing::error!(error = %e, "PTT release failed");
                        vec![format!("FAULT PTT release failed: {e}")]
                    }
                    Err(_) => vec!["FAULT PTT task panicked".into()],
                }
            }
        },

        // GRIDSQUARE is informational (stored for echo-back only). ARQBW and ARQTIMEOUT are stored
        // here and *applied by the worker loop* (bridge.rs) when an adaptive ARQ session is active
        // (`[ardop] enable_adaptive_arq = true`): ARQBW caps the adaptive ladder via
        // `set_arq_max_tx_level`, and ARQTIMEOUT drops an idle connection. With adaptive ARQ off
        // (the default, fixed-mode operation) there is no ladder/connection to bound, so they remain
        // accepted-and-echoed no-ops. See docs/dev/project/roadmap.md (TNC command-surface audit).
        "GRIDSQUARE" => {
            if let Some(grid) = parts.get(1).filter(|s| !s.is_empty()) {
                if !is_valid_gridsquare(grid) {
                    return vec![format!("FAULT invalid grid square: {grid}")];
                }
                let upper = grid.to_ascii_uppercase();
                *bridge.gridsquare.write().await = upper.clone();
                vec![format!("GRIDSQUARE {upper}")]
            } else {
                vec![format!("GRIDSQUARE {}", bridge.gridsquare.read().await)]
            }
        }

        "ARQBW" => {
            const VALID_BW: &[u16] = &[200, 500, 1000, 2000];
            match parts.get(1).filter(|s| !s.is_empty()) {
                None => vec![format!("ARQBW {}", bridge.arq_bw.read().await)],
                Some(arg) => match arg.parse::<u16>().ok().filter(|bw| VALID_BW.contains(bw)) {
                    Some(bw) => {
                        *bridge.arq_bw.write().await = bw;
                        vec![format!("ARQBW {bw}")]
                    }
                    None => vec![format!("FAULT invalid bandwidth: {arg}")],
                },
            }
        }

        "ARQTIMEOUT" => match parts.get(1).filter(|s| !s.is_empty()) {
            None => vec![format!("ARQTIMEOUT {}", bridge.arq_timeout.read().await)],
            Some(arg) => match arg.parse::<u16>() {
                Ok(secs) if (30..=600).contains(&secs) => {
                    *bridge.arq_timeout.write().await = secs;
                    vec![format!("ARQTIMEOUT {secs}")]
                }
                Ok(secs) => vec![format!("FAULT timeout out of range (30-600): {secs}")],
                Err(_) => vec![format!("FAULT invalid timeout: {arg}")],
            },
        },

        "CWID" => match parts.get(1).map(|s| s.to_uppercase()).as_deref() {
            Some("TRUE") => {
                bridge
                    .cwid_enabled
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                vec!["CWID TRUE".into()]
            }
            _ => {
                bridge
                    .cwid_enabled
                    .store(false, std::sync::atomic::Ordering::Relaxed);
                vec!["CWID FALSE".into()]
            }
        },

        "SENDID" => {
            // Request a one-shot ID; the worker keys PTT and sends it at the next frame boundary
            // (empty TX queue) — in the active mode, plus a Morse CW ID when CWID is on.
            bridge
                .id_requested
                .store(true, std::sync::atomic::Ordering::Relaxed);
            vec!["SENDID".into()]
        }

        "FECSEND" => {
            bridge
                .fec_tx
                .store(true, std::sync::atomic::Ordering::Relaxed);
            vec!["FECSEND".into()]
        }

        "FECRCV" => {
            bridge
                .fec_rx
                .store(true, std::sync::atomic::Ordering::Relaxed);
            vec!["FECRCV".into()]
        }

        "CONNECT_MESH" => {
            bridge
                .mesh_mode
                .store(true, std::sync::atomic::Ordering::Relaxed);
            vec!["CONNECT_MESH".into()]
        }

        "WAVEFORM" => {
            if let Some(new_mode) = parts.get(1).filter(|s| !s.is_empty()) {
                *bridge.mode.write().unwrap_or_else(|e| e.into_inner()) = new_mode.to_string();
                tracing::info!("waveform changed to {new_mode}");
                vec![format!("WAVEFORM {new_mode}")]
            } else {
                let mode = bridge
                    .mode
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                vec![format!("WAVEFORM {mode}")]
            }
        }

        "PING" => vec!["PONG".into()],

        "CLOSE" => vec![],

        _ => {
            tracing::warn!("unknown ARDOP command: {verb}");
            vec![]
        }
    }
}

/// Validates a Maidenhead grid locator: 4 chars (field+square) or 6 chars (+subsquare).
/// Format: two letters, two digits, optionally two letters (case-insensitive).
fn is_valid_gridsquare(s: &str) -> bool {
    let b = s.as_bytes();
    matches!(b.len(), 4 | 6)
        && b[0].is_ascii_alphabetic()
        && b[1].is_ascii_alphabetic()
        && b[2].is_ascii_digit()
        && b[3].is_ascii_digit()
        && (b.len() == 4 || (b[4].is_ascii_alphabetic() && b[5].is_ascii_alphabetic()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpulse_audio::LoopbackBackend;
    use openpulse_core::handshake::InMemoryTrustStore;
    use openpulse_modem::ModemEngine;
    use std::sync::atomic::Ordering;

    fn test_bridge() -> Arc<ModemBridge> {
        let engine = ModemEngine::new(Box::new(LoopbackBackend::default()));
        let (bridge, _rx) = ModemBridge::new(
            engine,
            "BPSK250".into(),
            false,
            InMemoryTrustStore::default(),
            None,
        );
        bridge
    }

    // `SENDID` now arms a real one-shot ID (was a warn-logged stub); the host response is unchanged,
    // so Pat/ARIM stay compatible — the command surface fulfils its contract instead of no-opping.
    #[tokio::test]
    async fn sendid_sets_the_oneshot_flag_and_keeps_the_response() {
        let bridge = test_bridge();
        assert!(!bridge.id_requested.load(Ordering::Relaxed));
        let resp = dispatch("SENDID", &bridge).await;
        assert_eq!(resp, vec!["SENDID".to_string()], "host response unchanged");
        assert!(
            bridge.id_requested.load(Ordering::Relaxed),
            "SENDID must arm the one-shot ID"
        );
    }

    #[tokio::test]
    async fn cwid_true_false_toggles_the_flag_and_keeps_the_response() {
        let bridge = test_bridge();
        assert!(!bridge.cwid_enabled.load(Ordering::Relaxed));

        let resp = dispatch("CWID TRUE", &bridge).await;
        assert_eq!(resp, vec!["CWID TRUE".to_string()]);
        assert!(
            bridge.cwid_enabled.load(Ordering::Relaxed),
            "CWID TRUE enables CW ID"
        );

        let resp = dispatch("CWID FALSE", &bridge).await;
        assert_eq!(resp, vec!["CWID FALSE".to_string()]);
        assert!(
            !bridge.cwid_enabled.load(Ordering::Relaxed),
            "CWID FALSE disables CW ID"
        );
    }

    #[tokio::test]
    async fn myid_mirrors_the_callsign_into_the_engine_for_the_regulatory_log() {
        let bridge = test_bridge();
        let resp = dispatch("MYID W1AW", &bridge).await;
        assert_eq!(
            resp,
            vec!["MYID W1AW".to_string()],
            "host response unchanged"
        );
        let engine_call = bridge
            .engine
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .callsign()
            .to_string();
        assert_eq!(
            engine_call, "W1AW",
            "MYID must mirror into the engine so the §97 TX-metadata log records it"
        );
    }

    // Regression guard for the engine-lock-across-the-executor hazard: the modem worker can hold the
    // engine std Mutex across a full RF burst. CONNECT/DISCONNECT must acquire that lock off the async
    // executor (spawn_blocking) so an in-flight CONNECT never parks the executor thread and starves an
    // unrelated command (here ABORT, which touches no engine lock). Run on a single-worker runtime and
    // observe from the (non-worker) test thread via a std channel, so the OLD inline-lock code fails
    // with a clean timeout instead of deadlocking the whole runtime.
    #[test]
    fn connect_holding_the_engine_lock_does_not_stall_an_abort() {
        use std::sync::mpsc;
        use std::time::Duration;

        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("runtime");
        let bridge = test_bridge();

        // A stand-in for the worker mid-burst: hold the engine lock on a std thread until released.
        let engine = bridge.engine.clone();
        let (locked_tx, locked_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let holder = std::thread::spawn(move || {
            let _guard = engine.lock().unwrap_or_else(|e| e.into_inner());
            locked_tx.send(()).expect("signal locked");
            let _ = release_rx.recv(); // hold across the "burst"
        });
        locked_rx.recv().expect("engine lock is held");

        // CONNECT needs the engine lock; kick it off and let the single worker pick it up. With the fix
        // it parks in the blocking pool (executor free); with the old inline lock it blocks the worker.
        let b_connect = bridge.clone();
        rt.spawn(async move {
            let _ = dispatch("CONNECT 500 K1ABC", &b_connect).await;
        });
        std::thread::sleep(Duration::from_millis(150)); // let the worker begin CONNECT

        // ABORT touches no engine lock — it must complete promptly on the freed executor.
        let (done_tx, done_rx) = mpsc::channel();
        let b_abort = bridge.clone();
        rt.spawn(async move {
            let out = dispatch("ABORT", &b_abort).await;
            let _ = done_tx.send(out);
        });

        let out = done_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("ABORT must not be blocked by a CONNECT holding the engine lock");
        assert_eq!(out, vec!["NEWSTATE DISC".to_string()]);

        release_tx.send(()).expect("release the held lock");
        holder.join().expect("holder thread");
        rt.shutdown_timeout(Duration::from_secs(1));
    }
}
