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
                let _ = engine.begin_secure_session(
                    SecureSessionParams {
                        local_minimum_mode: SigningMode::Normal,
                        peer_supported_modes: vec![SigningMode::Normal],
                        key_trust: PublicKeyTrustLevel::Unknown,
                        certificate_source: CertificateSource::OverAir,
                        psk_validated: false,
                    },
                    now_ms,
                );
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
                let _ = engine.end_secure_session(now_ms);
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
            Some("TRUE") => vec!["PTT TRUE".into()],
            _ => vec!["PTT FALSE".into()],
        },

        "GRIDSQUARE" => {
            if let Some(grid) = parts.get(1).filter(|s| !s.is_empty()) {
                *bridge.gridsquare.write().await = grid.to_string();
                vec![format!("GRIDSQUARE {grid}")]
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
                Ok(secs) => {
                    *bridge.arq_timeout.write().await = secs;
                    vec![format!("ARQTIMEOUT {secs}")]
                }
                Err(_) => vec![format!("FAULT invalid timeout: {arg}")],
            },
        },

        "CWID" => match parts.get(1).map(|s| s.to_uppercase()).as_deref() {
            Some("TRUE") => vec!["CWID TRUE".into()],
            _ => vec!["CWID FALSE".into()],
        },

        "SENDID" => vec!["SENDID".into()],

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

        "PING" => vec!["PONG".into()],

        "CLOSE" => vec![],

        _ => {
            tracing::warn!("unknown ARDOP command: {verb}");
            vec![]
        }
    }
}
