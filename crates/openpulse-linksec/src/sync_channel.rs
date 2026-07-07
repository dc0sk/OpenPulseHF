//! Blocking (std `Read`+`Write`) PSK Noise channel — used by the synchronous panel/CLI transport.
//!
//! Wire framing: each Noise message (handshake or transport) is a `u32` big-endian length prefix
//! followed by that many bytes. The channel is message-oriented (`send`/`recv`), matching the
//! control protocol's NDJSON lines and binary spectrum frames.

use std::io::{Read, Write};

use crate::{LinkSecError, NoiseHandshake, NoiseTransport, MAX_FRAME, PSK_LEN};

fn write_frame<S: Write>(stream: &mut S, data: &[u8]) -> Result<(), LinkSecError> {
    stream.write_all(&(data.len() as u32).to_be_bytes())?;
    stream.write_all(data)?;
    stream.flush()?;
    Ok(())
}

fn read_frame<S: Read>(stream: &mut S) -> Result<Vec<u8>, LinkSecError> {
    let mut len = [0u8; 4];
    stream.read_exact(&mut len)?;
    let n = u32::from_be_bytes(len) as usize;
    if n > MAX_FRAME {
        return Err(LinkSecError::FrameTooLarge(n));
    }
    let mut buf = vec![0u8; n];
    stream.read_exact(&mut buf)?;
    Ok(buf)
}

/// A PSK-authenticated, encrypted message channel over a blocking stream.
pub struct SyncNoise<S> {
    stream: S,
    transport: NoiseTransport,
}

impl<S: Read + Write> SyncNoise<S> {
    /// Perform the initiator (client) handshake over `stream`, then return the encrypted channel.
    pub fn initiator(mut stream: S, psk: &[u8; PSK_LEN]) -> Result<Self, LinkSecError> {
        let mut hs = NoiseHandshake::initiator(psk)?;
        write_frame(&mut stream, &hs.write_message()?)?; // -> e
        let m2 = read_frame(&mut stream)?; // <- e, ee
        hs.read_message(&m2)?;
        let transport = hs.into_transport()?;
        Ok(Self { stream, transport })
    }

    /// Perform the responder (server) handshake over `stream`. Fails on a PSK mismatch.
    pub fn responder(mut stream: S, psk: &[u8; PSK_LEN]) -> Result<Self, LinkSecError> {
        let mut hs = NoiseHandshake::responder(psk)?;
        let m1 = read_frame(&mut stream)?;
        hs.read_message(&m1)?; // wrong PSK fails here (bad auth tag)
        write_frame(&mut stream, &hs.write_message()?)?;
        let transport = hs.into_transport()?;
        Ok(Self { stream, transport })
    }

    /// Encrypt and send one application message.
    pub fn send(&mut self, msg: &[u8]) -> Result<(), LinkSecError> {
        let ct = self.transport.encrypt(msg)?;
        write_frame(&mut self.stream, &ct)
    }

    /// Receive and decrypt one application message.
    pub fn recv(&mut self) -> Result<Vec<u8>, LinkSecError> {
        let ct = read_frame(&mut self.stream)?;
        self.transport.decrypt(&ct)
    }

    /// Borrow the underlying stream (e.g. to set timeouts).
    pub fn get_ref(&self) -> &S {
        &self.stream
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};

    #[test]
    fn real_socket_handshake_and_round_trip() {
        let psk = [0x5Au8; PSK_LEN];
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let server = std::thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            let mut ch = SyncNoise::responder(sock, &psk).expect("responder handshake");
            let got = ch.recv().unwrap();
            assert_eq!(got, b"{\"cmd\":\"ptt\"}");
            ch.send(b"{\"ok\":true}").unwrap();
        });

        let sock = TcpStream::connect(addr).unwrap();
        let mut ch = SyncNoise::initiator(sock, &psk).expect("initiator handshake");
        ch.send(b"{\"cmd\":\"ptt\"}").unwrap();
        assert_eq!(ch.recv().unwrap(), b"{\"ok\":true}");
        server.join().unwrap();
    }

    #[test]
    fn wrong_psk_client_is_rejected() {
        let good = [1u8; PSK_LEN];
        let bad = [2u8; PSK_LEN];
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let server = std::thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            // A wrong-PSK client makes the responder handshake fail — the server drops it (fail closed).
            SyncNoise::responder(sock, &good).is_err()
        });

        // The client may complete its side or error depending on timing; the server's verdict is what
        // matters. Send a handshake with the wrong PSK.
        let sock = TcpStream::connect(addr).unwrap();
        let _ = SyncNoise::initiator(sock, &bad);
        assert!(
            server.join().unwrap(),
            "server must reject a wrong-PSK client"
        );
    }

    #[test]
    fn frame_helpers_round_trip_in_memory() {
        // A simple in-memory duplex to exercise framing without a socket.
        let mut buf: Vec<u8> = Vec::new();
        write_frame(&mut buf, b"hello").unwrap();
        let mut cur = std::io::Cursor::new(buf);
        assert_eq!(read_frame(&mut cur).unwrap(), b"hello");
    }
}
