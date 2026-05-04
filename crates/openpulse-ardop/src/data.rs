use std::sync::atomic::Ordering;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::bridge::ModemBridge;
use crate::error::ArdopError;

pub async fn serve(listener: TcpListener, bridge: Arc<ModemBridge>) -> Result<(), ArdopError> {
    loop {
        let (stream, addr) = listener.accept().await?;
        tracing::info!("data client connected: {addr}");
        let b = bridge.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, b).await {
                tracing::warn!("data client {addr} disconnected: {e}");
            }
        });
    }
}

async fn handle_client(
    stream: tokio::net::TcpStream,
    bridge: Arc<ModemBridge>,
) -> Result<(), ArdopError> {
    let (mut read_half, mut write_half) = stream.into_split();
    let mut rx_data = bridge.rx_data_tx.subscribe();
    let mut len_buf = [0u8; 2];

    loop {
        tokio::select! {
            r = read_half.read_exact(&mut len_buf) => {
                r?;
                let len = u16::from_be_bytes(len_buf) as usize;
                let mut payload = vec![0u8; len];
                read_half.read_exact(&mut payload).await?;
                bridge.tx_pending.fetch_add(len, Ordering::Relaxed);
                // Non-blocking send; silently drop if queue is full.
                bridge.tx_data_tx.try_send(payload).ok();
            }
            Ok(data) = rx_data.recv() => {
                let len = data.len() as u16;
                write_half.write_all(&len.to_be_bytes()).await?;
                write_half.write_all(&data).await?;
                write_half.flush().await?;
            }
        }
    }
}
