//! Pluggable connection transport for the panel.
//!
//! [`Transport`] abstracts over raw TCP (used by the native desktop build) and
//! WebSocket (used when connecting to the daemon's WS endpoint or when running
//! as a WASM browser application).  Both transports carry the same NDJSON text
//! messages and binary spectrum frames as the daemon protocol.

// TcpStream is only available on non-WASM targets.
#[cfg(not(target_arch = "wasm32"))]
use std::io::{BufRead, BufReader, Read, Write};
#[cfg(not(target_arch = "wasm32"))]
use std::net::TcpStream;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// A decoded message received from the daemon.
pub enum RecvMsg {
    /// A JSON-encoded `ControlEvent` line.
    Text(String),
    /// A raw binary spectrum frame (OPSP magic + bins).
    Binary(Vec<u8>),
}

/// Unified send/receive interface over TCP or WebSocket.
pub trait Transport: Send {
    /// Send a JSON command line to the daemon.  Returns `false` if the
    /// connection has broken.
    fn send_text(&mut self, s: &str) -> bool;
    /// Send raw binary data (e.g. a spectrum subscription payload).
    /// Kept on the trait because both native TCP and WASM transports expose
    /// the same spectrum frame API, even though some call sites only exercise
    /// the text path in a given build.
    #[allow(dead_code)]
    fn send_binary(&mut self, b: &[u8]) -> bool;
    /// Non-blocking poll for the next inbound message.  Returns `None` when no
    /// data is currently available, `Some(Err(()))` when the connection is
    /// closed or broken.
    fn try_recv(&mut self) -> Option<Result<RecvMsg, ()>>;
}

// ---------------------------------------------------------------------------
// TcpTransport — native only (no TCP sockets in WASM)
// ---------------------------------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
const SPECTRUM_MAGIC: u8 = b'O'; // first byte of "OPSP"

/// Resumable partial-frame reader state for the encrypted (Noise) TCP path — the panel polls
/// non-blocking, so a length-framed message may arrive across several `try_recv` calls.
#[cfg(not(target_arch = "wasm32"))]
struct NoiseCtx {
    transport: openpulse_linksec::NoiseTransport,
    /// Buffered ciphertext bytes not yet forming a complete frame.
    rx_buf: Vec<u8>,
    /// Expected frame body length once the 4-byte length prefix has been read.
    rx_expected: Option<usize>,
}

/// Read the control-channel PSK from `OPENPULSE_CONTROL_PSK` (64 hex chars = 32 bytes).
#[cfg(not(target_arch = "wasm32"))]
fn control_psk_from_env() -> Option<[u8; openpulse_linksec::PSK_LEN]> {
    let hex = std::env::var("OPENPULSE_CONTROL_PSK").ok()?;
    let hex = hex.trim();
    if hex.len() != openpulse_linksec::PSK_LEN * 2 {
        return None;
    }
    let mut out = [0u8; openpulse_linksec::PSK_LEN];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

#[cfg(not(target_arch = "wasm32"))]
fn framed_write(w: &mut impl Write, data: &[u8]) -> std::io::Result<()> {
    w.write_all(&(data.len() as u32).to_be_bytes())?;
    w.write_all(data)?;
    w.flush()
}

#[cfg(not(target_arch = "wasm32"))]
fn framed_read_blocking(r: &mut impl Read) -> std::io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let n = u32::from_be_bytes(len) as usize;
    if n > openpulse_linksec::MAX_FRAME {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "oversized handshake frame",
        ));
    }
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

/// Pull one complete `u32`-length-prefixed ciphertext frame out of the accumulation buffer, if a
/// whole one is present. Advances `rx_expected` as the prefix/body arrive across polls.
/// `Ok(None)` = still partial; `Err(())` = an oversized length was announced.
#[cfg(not(target_arch = "wasm32"))]
fn take_frame(
    rx_buf: &mut Vec<u8>,
    rx_expected: &mut Option<usize>,
) -> Result<Option<Vec<u8>>, ()> {
    if rx_expected.is_none() && rx_buf.len() >= 4 {
        let n = u32::from_be_bytes([rx_buf[0], rx_buf[1], rx_buf[2], rx_buf[3]]) as usize;
        if n > openpulse_linksec::MAX_FRAME {
            return Err(());
        }
        rx_buf.drain(0..4);
        *rx_expected = Some(n);
    }
    if let Some(n) = *rx_expected {
        if rx_buf.len() >= n {
            let frame: Vec<u8> = rx_buf.drain(0..n).collect();
            *rx_expected = None;
            return Ok(Some(frame));
        }
    }
    Ok(None)
}

/// Classify a decrypted message: an `OPSP` spectrum frame is binary; everything else is NDJSON text.
#[cfg(not(target_arch = "wasm32"))]
fn demux_message(pt: Vec<u8>) -> RecvMsg {
    if pt.len() >= 4 && &pt[0..4] == b"OPSP" {
        RecvMsg::Binary(pt)
    } else {
        RecvMsg::Text(String::from_utf8_lossy(&pt).trim().to_string())
    }
}

/// Raw TCP transport — matches the daemon's `TcpStream` connection loop. Encrypted with a PSK Noise
/// channel when `OPENPULSE_CONTROL_PSK` is set (REQ-SEC-CTL-01/02); plaintext otherwise.
#[cfg(not(target_arch = "wasm32"))]
pub struct TcpTransport {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    noise: Option<NoiseCtx>,
}

#[cfg(not(target_arch = "wasm32"))]
impl TcpTransport {
    /// Connect to `addr`; returns `None` on error. With `OPENPULSE_CONTROL_PSK` set, performs a PSK
    /// Noise initiator handshake before switching to the non-blocking poll.
    pub fn connect(addr: &str) -> Option<Self> {
        let stream = TcpStream::connect(addr).ok()?;
        let mut writer = stream.try_clone().ok()?;

        let noise = if let Some(psk) = control_psk_from_env() {
            // Handshake with a bounded blocking read timeout, over the raw stream.
            stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
            let mut hs = openpulse_linksec::NoiseHandshake::initiator(&psk).ok()?;
            framed_write(&mut writer, &hs.write_message().ok()?).ok()?; // -> e
            let mut read_stream: &TcpStream = &stream;
            let m2 = framed_read_blocking(&mut read_stream).ok()?; // <- e, ee
            hs.read_message(&m2).ok()?;
            Some(NoiseCtx {
                transport: hs.into_transport().ok()?,
                rx_buf: Vec::new(),
                rx_expected: None,
            })
        } else {
            None
        };

        stream
            .set_read_timeout(Some(Duration::from_millis(50)))
            .ok()?;
        Some(Self {
            reader: BufReader::new(stream),
            writer,
            noise,
        })
    }

    /// Non-blocking receive for the encrypted path: accumulate ciphertext across polls, decrypt a
    /// complete length-framed Noise message, then demux text vs binary.
    fn try_recv_noise(&mut self) -> Option<Result<RecvMsg, ()>> {
        let mut chunk = [0u8; 4096];
        let read_result = self.reader.get_mut().read(&mut chunk);
        let nc = self.noise.as_mut()?;
        match read_result {
            Ok(0) => return Some(Err(())), // EOF
            Ok(n) => nc.rx_buf.extend_from_slice(&chunk[..n]),
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => return Some(Err(())),
        }
        match take_frame(&mut nc.rx_buf, &mut nc.rx_expected) {
            Err(()) => Some(Err(())),
            Ok(Some(ct)) => match nc.transport.decrypt(&ct) {
                Ok(pt) => Some(Ok(demux_message(pt))),
                Err(_) => Some(Err(())),
            },
            Ok(None) => None,
        }
    }

    /// Non-blocking receive for the plaintext path (line-delimited NDJSON + `OPSP` binary frames).
    fn try_recv_plain(&mut self) -> Option<Result<RecvMsg, ()>> {
        // Peek at the first buffered byte to distinguish binary from NDJSON.
        let first = match self.reader.fill_buf() {
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                return None;
            }
            Err(_) => return Some(Err(())),
            Ok(&[]) => return Some(Err(())), // EOF
            Ok(buf) => buf[0],
        };

        if first == SPECTRUM_MAGIC {
            // Binary spectrum frame: 4 magic + 2 fft_size LE + 4 sample_rate LE + bins.
            let mut header = [0u8; 10];
            if self.reader.read_exact(&mut header).is_err() {
                return Some(Err(()));
            }
            // Validate full 4-byte magic before trusting the rest of the header.
            if &header[0..4] != b"OPSP" {
                return Some(Err(()));
            }
            let fft_size = u16::from_le_bytes([header[4], header[5]]) as usize;
            // Sanity-cap to prevent large allocations from a malformed frame.
            if fft_size > 8192 {
                return Some(Err(()));
            }
            let mut payload = vec![0u8; fft_size * 4];
            if self.reader.read_exact(&mut payload).is_err() {
                return Some(Err(()));
            }
            let mut frame = Vec::with_capacity(10 + payload.len());
            frame.extend_from_slice(&header);
            frame.extend_from_slice(&payload);
            Some(Ok(RecvMsg::Binary(frame)))
        } else {
            let mut line = String::new();
            match self.reader.read_line(&mut line) {
                Err(ref e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    None
                }
                Err(_) | Ok(0) => Some(Err(())),
                Ok(_) => {
                    let text = line.trim().to_string();
                    if text.is_empty() {
                        None
                    } else {
                        Some(Ok(RecvMsg::Text(text)))
                    }
                }
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Transport for TcpTransport {
    fn send_text(&mut self, s: &str) -> bool {
        if let Some(nc) = self.noise.as_mut() {
            match nc.transport.encrypt(s.as_bytes()) {
                Ok(ct) => framed_write(&mut self.writer, &ct).is_ok(),
                Err(_) => false,
            }
        } else {
            writeln!(self.writer, "{s}").is_ok() && self.writer.flush().is_ok()
        }
    }

    fn send_binary(&mut self, b: &[u8]) -> bool {
        if let Some(nc) = self.noise.as_mut() {
            match nc.transport.encrypt(b) {
                Ok(ct) => framed_write(&mut self.writer, &ct).is_ok(),
                Err(_) => false,
            }
        } else {
            self.writer.write_all(b).is_ok() && self.writer.flush().is_ok()
        }
    }

    fn try_recv(&mut self) -> Option<Result<RecvMsg, ()>> {
        if self.noise.is_some() {
            self.try_recv_noise()
        } else {
            self.try_recv_plain()
        }
    }
}

// ---------------------------------------------------------------------------
// WsTransport
// ---------------------------------------------------------------------------

/// WebSocket transport — works on native desktop and (when compiled to WASM)
/// in the browser.  Connects to a `ws://` or `wss://` URL.
pub struct WsTransport {
    sender: ewebsock::WsSender,
    receiver: ewebsock::WsReceiver,
    connected: bool,
}

impl WsTransport {
    /// Connect to `url` (e.g. `"ws://127.0.0.1:9001"`).  Returns `None` if
    /// the connection attempt fails synchronously.
    pub fn connect(url: &str) -> Option<Self> {
        let options = ewebsock::Options::default();
        let (sender, receiver) = ewebsock::connect(url, options).ok()?;
        Some(Self {
            sender,
            receiver,
            connected: true,
        })
    }
}

// On WASM, WsSender uses Rc<WebSocket> which is not Send; Transport is only
// used from the native background thread, so skip this impl for WASM.
#[cfg(not(target_arch = "wasm32"))]
impl Transport for WsTransport {
    fn send_text(&mut self, s: &str) -> bool {
        if !self.connected {
            return false;
        }
        self.sender.send(ewebsock::WsMessage::Text(s.to_string()));
        true
    }

    fn send_binary(&mut self, b: &[u8]) -> bool {
        if !self.connected {
            return false;
        }
        self.sender.send(ewebsock::WsMessage::Binary(b.to_vec()));
        true
    }

    fn try_recv(&mut self) -> Option<Result<RecvMsg, ()>> {
        if !self.connected {
            return Some(Err(()));
        }
        match self.receiver.try_recv() {
            None => None,
            Some(ewebsock::WsEvent::Opened) => None,
            Some(ewebsock::WsEvent::Message(ewebsock::WsMessage::Text(s))) => {
                Some(Ok(RecvMsg::Text(s)))
            }
            Some(ewebsock::WsEvent::Message(ewebsock::WsMessage::Binary(b))) => {
                Some(Ok(RecvMsg::Binary(b)))
            }
            Some(ewebsock::WsEvent::Message(_)) => None,
            Some(ewebsock::WsEvent::Error(e)) => {
                tracing::warn!("WebSocket error: {e}");
                self.connected = false;
                Some(Err(()))
            }
            Some(ewebsock::WsEvent::Closed) => {
                self.connected = false;
                Some(Err(()))
            }
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn take_frame_assembles_across_partial_reads() {
        let mut buf: Vec<u8> = Vec::new();
        let mut exp: Option<usize> = None;

        // Length prefix arrives in two chunks; body arrives in two chunks.
        buf.extend_from_slice(&[0, 0]);
        assert_eq!(take_frame(&mut buf, &mut exp), Ok(None)); // partial prefix
        buf.extend_from_slice(&[0, 5]); // now a complete len=5 prefix
        assert_eq!(take_frame(&mut buf, &mut exp), Ok(None)); // prefix parsed, body empty
        assert_eq!(exp, Some(5));
        buf.extend_from_slice(b"hel");
        assert_eq!(take_frame(&mut buf, &mut exp), Ok(None)); // partial body
        buf.extend_from_slice(b"lo");
        assert_eq!(take_frame(&mut buf, &mut exp), Ok(Some(b"hello".to_vec())));
        assert_eq!(exp, None);

        // Two back-to-back frames delivered in one chunk.
        buf.extend_from_slice(&[0, 0, 0, 2, 0xAA, 0xBB, 0, 0, 0, 1, 0x11]);
        assert_eq!(take_frame(&mut buf, &mut exp), Ok(Some(vec![0xAA, 0xBB])));
        assert_eq!(take_frame(&mut buf, &mut exp), Ok(Some(vec![0x11])));
        assert_eq!(take_frame(&mut buf, &mut exp), Ok(None));
    }

    #[test]
    fn take_frame_rejects_oversized_length() {
        let mut buf: Vec<u8> = vec![0xFF, 0xFF, 0xFF, 0xFF];
        let mut exp: Option<usize> = None;
        assert_eq!(take_frame(&mut buf, &mut exp), Err(()));
    }

    #[test]
    fn demux_classifies_opsp_and_text() {
        assert!(matches!(
            demux_message(b"OPSP\x00\x01".to_vec()),
            RecvMsg::Binary(_)
        ));
        match demux_message(b"{\"type\":\"metrics\"}".to_vec()) {
            RecvMsg::Text(s) => assert!(s.contains("metrics")),
            _ => panic!("json should demux as text"),
        }
    }
}
