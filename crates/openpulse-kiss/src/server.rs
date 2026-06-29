//! TCP listener and per-client KISS frame handler.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::broadcast::error::RecvError;

use crate::bridge::KissBridge;
use crate::error::KissTncError;
use crate::kiss;

/// Maximum AX.25/KISS payload size accepted by ModemEngine frame layer.
const MAX_PAYLOAD_BYTES: usize = 255;
/// Maximum raw frame body size (type byte + worst-case-escaped payload).
/// 255 bytes × 2 (FESC escaping) + 1 type byte = 511; round up with margin.
const MAX_FRAME_BODY: usize = 600;

pub async fn serve(listener: TcpListener, bridge: Arc<KissBridge>) -> Result<(), KissTncError> {
    loop {
        let (stream, addr) = listener.accept().await?;
        tracing::info!("KISS client connected: {addr}");
        let b = bridge.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, b).await {
                tracing::warn!("KISS client {addr} disconnected: {e}");
            }
        });
    }
}

async fn handle_client(
    stream: tokio::net::TcpStream,
    bridge: Arc<KissBridge>,
) -> Result<(), KissTncError> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut rx_data = bridge.rx_data_tx.subscribe();

    loop {
        tokio::select! {
            result = read_kiss_frame(&mut reader) => {
                let frame_body = result?;
                if frame_body.is_empty() {
                    continue;
                }
                match kiss::decode(&frame_body) {
                    Ok((kiss::KISS_DATA, payload)) => {
                        if payload.len() > MAX_PAYLOAD_BYTES {
                            tracing::warn!(
                                "KISS frame too large ({} B > {MAX_PAYLOAD_BYTES} B), dropping",
                                payload.len()
                            );
                            continue;
                        }
                        let len = payload.len();
                        // Only track bytes we actually enqueue.
                        if bridge.tx_data_tx.try_send(payload).is_ok() {
                            bridge.tx_pending.fetch_add(len, Ordering::Relaxed);
                        }
                    }
                    Ok((cmd, _)) => {
                        // KISS control frames (TXDELAY 0x01, P 0x02, SlotTime 0x03, TXtail 0x04,
                        // FullDuplex 0x05, SetHardware 0x06) are advisory PTT/CSMA-timing hints that
                        // a host MAY send; this TNC manages PTT/channel access itself and does not
                        // apply them. Log so the no-op is visible rather than silent. (See the
                        // 2026-06-27 TNC command-surface audit in docs/dev/steering/roadmap.md.)
                        tracing::debug!(kiss_cmd = format!("0x{cmd:02x}"), "ignoring KISS control frame (not applied)");
                    }
                    Err(e) => {
                        tracing::debug!("KISS decode error: {e}");
                    }
                }
            }
            result = rx_data.recv() => {
                match result {
                    Ok(payload) => {
                        let frame = kiss::encode(kiss::KISS_DATA, &payload);
                        write_half.write_all(&frame).await?;
                        write_half.flush().await?;
                    }
                    Err(RecvError::Lagged(n)) => {
                        // Slow client; frames were dropped from the broadcast
                        // ring buffer.  Log and continue — the client stays connected.
                        tracing::warn!("KISS RX lagged, {n} frame(s) dropped for this client");
                    }
                    Err(RecvError::Closed) => {
                        return Ok(());
                    }
                }
            }
        }
    }
}

/// Read bytes from `reader` until a complete KISS frame body is available.
///
/// Skips leading bytes before the first FEND, then collects bytes until the
/// next FEND.  Returns the raw frame body (type byte + escaped payload).
async fn read_kiss_frame(
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
) -> Result<Vec<u8>, KissTncError> {
    // Skip forward to the first FEND delimiter.
    loop {
        let b = reader.read_u8().await?;
        if b == kiss::FEND {
            break;
        }
    }
    // Accumulate until the next FEND, bounded by MAX_FRAME_BODY.
    let mut buf = Vec::new();
    loop {
        let b = reader.read_u8().await?;
        if b == kiss::FEND {
            return Ok(buf);
        }
        buf.push(b);
        if buf.len() > MAX_FRAME_BODY {
            return Err(KissTncError::FrameTooLarge {
                len: buf.len(),
                max: MAX_FRAME_BODY,
            });
        }
    }
}
