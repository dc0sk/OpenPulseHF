use std::sync::atomic::Ordering;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::broadcast::error::RecvError;

use crate::bridge::ModemBridge;
use crate::error::ArdopError;

/// Maximum data-port frame payload accepted from clients.
///
/// A host block can exceed one modem frame (#1385), so it is split into frame-sized chunks before
/// it reaches the modem worker. 4096 bytes bounds what one block may allocate, so a crafted
/// `u16::MAX` length prefix cannot force a 64 KiB heap allocation.
const MAX_FRAME_BYTES: usize = 4096;

/// Split one host data block into chunks one modem frame carries, in order (#1385).
///
/// The data port is a byte stream to the host on both ends (Pat concatenates what the TNC delivers),
/// so chunking at the sender needs no reassembly at the receiver: each chunk is an ordinary frame.
/// Before this a block over 255 bytes reached `Frame::new` whole and was refused, so it never went on
/// the air and the host saw no error it could read as "too large".
pub(crate) fn frame_chunks(block: Vec<u8>) -> Vec<Vec<u8>> {
    if block.len() <= openpulse_core::frame::Frame::MAX_PAYLOAD {
        return vec![block];
    }
    block
        .chunks(openpulse_core::frame::Frame::MAX_PAYLOAD)
        .map(<[u8]>::to_vec)
        .collect()
}

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
                if len > MAX_FRAME_BYTES {
                    tracing::warn!(len, max = MAX_FRAME_BYTES, "data port frame rejected — too large");
                    return Err(ArdopError::FrameTooLarge { len, max: MAX_FRAME_BYTES });
                }
                let mut payload = vec![0u8; len];
                read_half.read_exact(&mut payload).await?;
                bridge.tx_pending.fetch_add(len, Ordering::Relaxed);
                // Apply backpressure instead of dropping: the SyncSender blocks when the modem worker's
                // queue is full, throttling this client's TCP reader — so a >64-frame burst (a normal
                // Winlink message) is delivered in full rather than silently truncated. `spawn_blocking`
                // keeps the blocking send off the async reactor. `Err` means the worker is gone → close.
                let tx = bridge.tx_data_tx.clone();
                let chunks = frame_chunks(payload);
                if tokio::task::spawn_blocking(move || chunks.into_iter().try_for_each(|c| tx.send(c)))
                    .await
                    .map_err(|_| ())
                    .and_then(|r| r.map_err(|_| ()))
                    .is_err()
                {
                    tracing::warn!("ARDOP data port: modem worker gone — closing data client");
                    bridge.tx_pending.fetch_sub(
                        len.min(bridge.tx_pending.load(Ordering::Relaxed)),
                        Ordering::Relaxed,
                    );
                    return Ok(());
                }
            }
            result = rx_data.recv() => {
                match result {
                    Ok(data) => {
                        let len = data.len() as u16;
                        write_half.write_all(&len.to_be_bytes()).await?;
                        write_half.write_all(&data).await?;
                        write_half.flush().await?;
                    }
                    Err(RecvError::Lagged(n)) => {
                        // Slow client; frames were dropped from the broadcast ring. Log and continue
                        // rather than stalling the receive loop (the old `Ok(data) =` pattern silently
                        // disabled this branch on a Lagged error).
                        tracing::warn!("ARDOP data RX lagged, {n} frame(s) dropped for this client");
                    }
                    Err(RecvError::Closed) => return Ok(()),
                }
            }
        }
    }
}

#[cfg(test)]
mod chunk_tests {
    use super::frame_chunks;
    use openpulse_core::frame::Frame;

    #[test]
    fn a_block_within_one_frame_is_one_chunk() {
        assert_eq!(frame_chunks(vec![7; 255]), vec![vec![7; 255]]);
        assert_eq!(frame_chunks(Vec::new()), vec![Vec::<u8>::new()]);
    }

    /// #1385: a 4 096-byte host block becomes frame-sized chunks, in order, every one of which
    /// `Frame::new` accepts — before this the whole block reached `Frame::new` and was refused.
    #[test]
    fn a_large_block_splits_into_frames_the_engine_accepts() {
        let block: Vec<u8> = (0..4096u32).map(|i| i as u8).collect();
        let chunks = frame_chunks(block.clone());
        assert_eq!(chunks.len(), 4096usize.div_ceil(Frame::MAX_PAYLOAD));
        assert!(chunks.iter().all(|c| Frame::new(0, c.clone()).is_ok()));
        assert_eq!(chunks.concat(), block, "order and content preserved");
    }
}
