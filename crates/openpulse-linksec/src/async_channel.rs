//! Async (tokio) PSK Noise channel — for the daemon control server. Same `u32`-BE-length framing as
//! [`crate::sync_channel`]. [`AsyncNoise::into_split`] yields concurrently-usable halves (the daemon's
//! `select!` loop writes events while reading commands) that share the transport via a brief per-message
//! lock — Noise's send/recv nonces are independent, so this is safe.

use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;

use crate::{LinkSecError, NoiseHandshake, NoiseTransport, MAX_FRAME, PSK_LEN};

async fn write_frame<S: AsyncWrite + Unpin>(s: &mut S, data: &[u8]) -> Result<(), LinkSecError> {
    s.write_all(&(data.len() as u32).to_be_bytes()).await?;
    s.write_all(data).await?;
    s.flush().await?;
    Ok(())
}

async fn read_frame<S: AsyncRead + Unpin>(s: &mut S) -> Result<Vec<u8>, LinkSecError> {
    let mut len = [0u8; 4];
    s.read_exact(&mut len).await?;
    let n = u32::from_be_bytes(len) as usize;
    if n > MAX_FRAME {
        return Err(LinkSecError::FrameTooLarge(n));
    }
    let mut buf = vec![0u8; n];
    s.read_exact(&mut buf).await?;
    Ok(buf)
}

/// A PSK-authenticated, encrypted message channel over an async stream.
pub struct AsyncNoise<S> {
    stream: S,
    transport: NoiseTransport,
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncNoise<S> {
    /// Perform the initiator (client) handshake over `stream`.
    pub async fn initiator(mut stream: S, psk: &[u8; PSK_LEN]) -> Result<Self, LinkSecError> {
        let mut hs = NoiseHandshake::initiator(psk)?;
        write_frame(&mut stream, &hs.write_message()?).await?;
        let m2 = read_frame(&mut stream).await?;
        hs.read_message(&m2)?;
        let transport = hs.into_transport()?;
        Ok(Self { stream, transport })
    }

    /// Perform the responder (server) handshake over `stream`. Fails on a PSK mismatch.
    pub async fn responder(mut stream: S, psk: &[u8; PSK_LEN]) -> Result<Self, LinkSecError> {
        let mut hs = NoiseHandshake::responder(psk)?;
        let m1 = read_frame(&mut stream).await?;
        hs.read_message(&m1)?;
        write_frame(&mut stream, &hs.write_message()?).await?;
        let transport = hs.into_transport()?;
        Ok(Self { stream, transport })
    }

    /// Encrypt and send one application message.
    pub async fn send(&mut self, msg: &[u8]) -> Result<(), LinkSecError> {
        let ct = self.transport.encrypt(msg)?;
        write_frame(&mut self.stream, &ct).await
    }

    /// Receive and decrypt one application message.
    pub async fn recv(&mut self) -> Result<Vec<u8>, LinkSecError> {
        let ct = read_frame(&mut self.stream).await?;
        self.transport.decrypt(&ct)
    }

    /// Split into concurrently-usable write/read halves sharing the transport.
    #[allow(clippy::type_complexity)]
    pub fn into_split(
        self,
    ) -> (
        NoiseWriteHalf<tokio::io::WriteHalf<S>>,
        NoiseReadHalf<tokio::io::ReadHalf<S>>,
    ) {
        let (r, w) = tokio::io::split(self.stream);
        let t = Arc::new(Mutex::new(self.transport));
        (
            NoiseWriteHalf {
                w,
                t: Arc::clone(&t),
            },
            NoiseReadHalf { r, t },
        )
    }
}

/// The write half of a split [`AsyncNoise`] channel.
pub struct NoiseWriteHalf<W> {
    w: W,
    t: Arc<Mutex<NoiseTransport>>,
}

impl<W: AsyncWrite + Unpin> NoiseWriteHalf<W> {
    /// Encrypt and send one message.
    pub async fn send(&mut self, msg: &[u8]) -> Result<(), LinkSecError> {
        let ct = {
            let mut g = self.t.lock().await;
            g.encrypt(msg)?
        };
        write_frame(&mut self.w, &ct).await
    }
}

/// The read half of a split [`AsyncNoise`] channel.
pub struct NoiseReadHalf<R> {
    r: R,
    t: Arc<Mutex<NoiseTransport>>,
}

impl<R: AsyncRead + Unpin> NoiseReadHalf<R> {
    /// Receive and decrypt one message.
    pub async fn recv(&mut self) -> Result<Vec<u8>, LinkSecError> {
        let ct = read_frame(&mut self.r).await?;
        let mut g = self.t.lock().await;
        g.decrypt(&ct)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::{TcpListener, TcpStream};

    #[tokio::test]
    async fn real_tcp_handshake_and_round_trip() {
        let psk = [0x33u8; PSK_LEN];
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let srv = tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            let mut ch = AsyncNoise::responder(sock, &psk).await.expect("responder");
            assert_eq!(ch.recv().await.unwrap(), b"cmd");
            ch.send(b"ok").await.unwrap();
        });
        let sock = TcpStream::connect(addr).await.unwrap();
        let mut ch = AsyncNoise::initiator(sock, &psk).await.expect("initiator");
        ch.send(b"cmd").await.unwrap();
        assert_eq!(ch.recv().await.unwrap(), b"ok");
        srv.await.unwrap();
    }

    #[tokio::test]
    async fn wrong_psk_is_rejected() {
        let good = [1u8; PSK_LEN];
        let bad = [2u8; PSK_LEN];
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let srv = tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            AsyncNoise::responder(sock, &good).await.is_err()
        });
        let sock = TcpStream::connect(addr).await.unwrap();
        let _ = AsyncNoise::initiator(sock, &bad).await;
        assert!(srv.await.unwrap(), "server must reject a wrong-PSK client");
    }

    #[tokio::test]
    async fn split_halves_round_trip() {
        let psk = [7u8; PSK_LEN];
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let srv = tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            let ch = AsyncNoise::responder(sock, &psk).await.unwrap();
            let (mut w, mut r) = ch.into_split();
            assert_eq!(r.recv().await.unwrap(), b"hi");
            w.send(b"yo").await.unwrap();
        });
        let sock = TcpStream::connect(addr).await.unwrap();
        let ch = AsyncNoise::initiator(sock, &psk).await.unwrap();
        let (mut w, mut r) = ch.into_split();
        w.send(b"hi").await.unwrap();
        assert_eq!(r.recv().await.unwrap(), b"yo");
        srv.await.unwrap();
    }
}
