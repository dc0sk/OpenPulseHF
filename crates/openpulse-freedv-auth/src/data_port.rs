//! Async UDP connection to the FreeDV Qt-GUI data port.
//!
//! FreeDV GUI ≥ 1.6 exposes a UDP data port (default `127.0.0.1:10001`).
//! Bytes written to this port are injected into the FreeDV data channel and
//! transmitted alongside voice.  Bytes received from the port are data frames
//! received from the air.

use std::net::SocketAddr;
use thiserror::Error;
use tokio::net::UdpSocket;

#[derive(Debug, Error)]
pub enum DataPortError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Async UDP connection to the FreeDV data port.
pub struct FreeDvDataPort {
    socket: UdpSocket,
    peer: SocketAddr,
}

impl FreeDvDataPort {
    /// Bind a local UDP socket and associate it with the FreeDV data port address.
    pub async fn connect(peer_addr: &str) -> Result<Self, DataPortError> {
        let peer: SocketAddr = peer_addr.parse().map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("bad addr: {e}"))
        })?;
        let local = if peer.is_ipv6() {
            "[::]:0"
        } else {
            "0.0.0.0:0"
        };
        let socket = UdpSocket::bind(local).await?;
        socket.connect(peer).await?;
        Ok(Self { socket, peer })
    }

    /// Send `data` to the FreeDV data port.
    pub async fn send(&self, data: &[u8]) -> Result<(), DataPortError> {
        self.socket.send(data).await?;
        Ok(())
    }

    /// Receive one UDP datagram into `buf`.  Returns the number of bytes written.
    pub async fn recv(&self, buf: &mut [u8]) -> Result<usize, DataPortError> {
        Ok(self.socket.recv(buf).await?)
    }

    /// The remote address this port sends to.
    pub fn peer_addr(&self) -> SocketAddr {
        self.peer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::UdpSocket;

    #[tokio::test]
    async fn send_recv_loopback() {
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server_addr = server.local_addr().unwrap().to_string();

        let port = FreeDvDataPort::connect(&server_addr).await.unwrap();
        port.send(b"hello freedv").await.unwrap();

        let mut buf = [0u8; 64];
        let (n, _) = server.recv_from(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"hello freedv");
    }
}
