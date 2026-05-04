//! TCP listener and per-client KISS frame handler.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use crate::bridge::KissBridge;
use crate::error::KissTncError;
use crate::kiss;

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
                        let len = payload.len();
                        bridge.tx_pending.fetch_add(len, Ordering::Relaxed);
                        bridge.tx_data_tx.try_send(payload).ok();
                    }
                    Ok(_) => {
                        // Non-data KISS commands (e.g. SETHARDWARE) — ignore.
                    }
                    Err(e) => {
                        tracing::debug!("KISS decode error: {e}");
                    }
                }
            }
            Ok(payload) = rx_data.recv() => {
                let frame = kiss::encode(kiss::KISS_DATA, &payload);
                write_half.write_all(&frame).await?;
                write_half.flush().await?;
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
    // Accumulate until the next FEND.
    let mut buf = Vec::new();
    loop {
        let b = reader.read_u8().await?;
        if b == kiss::FEND {
            return Ok(buf);
        }
        buf.push(b);
    }
}
