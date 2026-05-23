use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use openpulse_core::trust::{CertificateSource, PublicKeyTrustLevel, SigningMode};
use openpulse_modem::engine::SecureSessionParams;

use crate::bridge::ModemBridge;
use crate::error::ArdopError;
use crate::state::TncState;

/// Maximum command line length accepted before dropping the connection.
const MAX_CMD_LINE: usize = 4096;

pub async fn serve(listener: TcpListener, bridge: Arc<ModemBridge>) -> Result<(), ArdopError> {
    loop {
        let (stream, addr) = listener.accept().await?;
        tracing::info!("command client connected: {addr}");
        let b = bridge.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, b).await {
                tracing::warn!("command client {addr} disconnected: {e}");
            }
        });
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
            n = reader.read_line(&mut line) => {
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
            Ok(event) = event_rx.recv() => {
                write_half.write_all(event.as_bytes()).await?;
                write_half.write_all(b"\r\n").await?;
                write_half.flush().await?;
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
            if let Ok(mut engine) = bridge.engine.lock() {
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
                    tracing::warn!(peer = %peer, error = %e, "begin_secure_session failed on CONNECT");
                }
            }
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
            if let Ok(mut engine) = bridge.engine.lock() {
                if let Err(e) = engine.end_secure_session(now_ms) {
                    tracing::warn!(error = %e, "end_secure_session failed on DISCONNECT");
                }
            }
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
                match bridge
                    .ptt
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .assert_ptt()
                {
                    Ok(()) => vec!["PTT TRUE".into()],
                    Err(e) => {
                        tracing::error!(error = %e, "PTT assert failed");
                        vec![format!("FAULT PTT assert failed: {e}")]
                    }
                }
            }
            _ => {
                match bridge
                    .ptt
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .release_ptt()
                {
                    Ok(()) => vec!["PTT FALSE".into()],
                    Err(e) => {
                        tracing::error!(error = %e, "PTT release failed");
                        vec![format!("FAULT PTT release failed: {e}")]
                    }
                }
            }
        },

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
                tracing::warn!("CWID TRUE received but CW ID transmission is not implemented; stub response only");
                vec!["CWID TRUE".into()]
            }
            _ => vec!["CWID FALSE".into()],
        },

        "SENDID" => {
            tracing::warn!(
                "SENDID received but ID frame transmission is not implemented; stub response only"
            );
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
