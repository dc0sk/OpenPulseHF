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

/// Raw TCP transport — matches the existing `TcpStream` connection loop.
#[cfg(not(target_arch = "wasm32"))]
pub struct TcpTransport {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

#[cfg(not(target_arch = "wasm32"))]
impl TcpTransport {
    /// Connect to `addr`; returns `None` on error.
    pub fn connect(addr: &str) -> Option<Self> {
        let stream = TcpStream::connect(addr).ok()?;
        stream
            .set_read_timeout(Some(Duration::from_millis(50)))
            .ok()?;
        let writer = stream.try_clone().ok()?;
        Some(Self {
            reader: BufReader::new(stream),
            writer,
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Transport for TcpTransport {
    fn send_text(&mut self, s: &str) -> bool {
        writeln!(self.writer, "{s}").is_ok() && self.writer.flush().is_ok()
    }

    fn send_binary(&mut self, b: &[u8]) -> bool {
        self.writer.write_all(b).is_ok() && self.writer.flush().is_ok()
    }

    fn try_recv(&mut self) -> Option<Result<RecvMsg, ()>> {
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
