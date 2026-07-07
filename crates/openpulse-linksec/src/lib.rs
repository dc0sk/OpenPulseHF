//! Control-channel link security (REQ-SEC-CTL-01/02).
//!
//! A pre-shared-key authenticated, encrypted channel for the daemon ↔ client control link, plus the
//! non-loopback auth gate. The channel uses the **Noise protocol** (`Noise_NNpsk0`, X25519 +
//! ChaCha20-Poly1305 + BLAKE2s) via `snow`: both endpoints prove knowledge of the 32-byte PSK during
//! the handshake (mutual authentication) and then exchange AEAD-encrypted messages with forward
//! secrecy. rustls is not used because it has no external/raw TLS-PSK support; OpenSSL (K4remote's
//! choice) would add a C dependency to an otherwise pure-Rust workspace.
//!
//! Layers: [`NoiseHandshake`]/[`NoiseTransport`] are the transport-agnostic byte-buffer core;
//! [`sync_channel::SyncNoise`] (blocking, for the panel/CLI) and [`async_channel::AsyncNoise`]
//! (tokio, `tokio` feature, for the daemon — with `into_split` for concurrent read/write) add the
//! `u32`-length-framed socket channel on top. Wiring these into the daemon connection loop + panel
//! transport is a separate, live-validated step.

use thiserror::Error;

/// The Noise pattern: no static keys, PSK mixed at position 0 → mutual auth from the PSK alone.
const NOISE_PATTERN: &str = "Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s";

/// Length of the pre-shared key, in bytes.
pub const PSK_LEN: usize = 32;

/// Max plaintext per [`NoiseTransport::encrypt`] call (Noise message limit minus the 16-byte tag).
pub const MAX_PLAINTEXT: usize = 65535 - 16;

/// Max encrypted frame length accepted on the wire (plaintext + tag, plus a small margin).
pub const MAX_FRAME: usize = MAX_PLAINTEXT + 16;

/// Errors from the link-security layer.
#[derive(Debug, Error)]
pub enum LinkSecError {
    #[error("noise protocol error: {0}")]
    Noise(String),
    #[error("message too large ({0} > {MAX_PLAINTEXT} bytes)")]
    TooLarge(usize),
    #[error("link io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("oversized frame on the wire ({0} > {MAX_FRAME} bytes)")]
    FrameTooLarge(usize),
}

#[cfg(feature = "tokio")]
pub mod async_channel;
pub mod sync_channel;

fn noise(e: snow::Error) -> LinkSecError {
    LinkSecError::Noise(e.to_string())
}

/// Drives the Noise handshake for one endpoint. Feed peer messages to [`read_message`] and send the
/// output of [`write_message`] until [`is_finished`], then convert with [`into_transport`].
///
/// NN handshake (2 messages): initiator `write` → responder `read` + `write` → initiator `read`.
///
/// [`read_message`]: NoiseHandshake::read_message
/// [`write_message`]: NoiseHandshake::write_message
/// [`is_finished`]: NoiseHandshake::is_finished
/// [`into_transport`]: NoiseHandshake::into_transport
pub struct NoiseHandshake {
    state: snow::HandshakeState,
}

impl NoiseHandshake {
    /// Build the initiator side from the shared 32-byte PSK.
    pub fn initiator(psk: &[u8; PSK_LEN]) -> Result<Self, LinkSecError> {
        Self::build(psk, true)
    }

    /// Build the responder side from the shared 32-byte PSK.
    pub fn responder(psk: &[u8; PSK_LEN]) -> Result<Self, LinkSecError> {
        Self::build(psk, false)
    }

    fn build(psk: &[u8; PSK_LEN], initiator: bool) -> Result<Self, LinkSecError> {
        let params = NOISE_PATTERN.parse().map_err(noise)?;
        let builder = snow::Builder::new(params).psk(0, psk);
        let state = if initiator {
            builder.build_initiator().map_err(noise)?
        } else {
            builder.build_responder().map_err(noise)?
        };
        Ok(Self { state })
    }

    /// Produce this endpoint's next handshake message to send to the peer.
    pub fn write_message(&mut self) -> Result<Vec<u8>, LinkSecError> {
        let mut buf = vec![0u8; 1024];
        let n = self.state.write_message(&[], &mut buf).map_err(noise)?;
        buf.truncate(n);
        Ok(buf)
    }

    /// Consume a handshake message received from the peer. Fails on a PSK mismatch (bad auth tag).
    pub fn read_message(&mut self, msg: &[u8]) -> Result<(), LinkSecError> {
        let mut buf = vec![0u8; 1024];
        self.state.read_message(msg, &mut buf).map_err(noise)?;
        Ok(())
    }

    /// Whether the handshake is complete.
    pub fn is_finished(&self) -> bool {
        self.state.is_handshake_finished()
    }

    /// Convert the finished handshake into the encrypted transport.
    pub fn into_transport(self) -> Result<NoiseTransport, LinkSecError> {
        let state = self.state.into_transport_mode().map_err(noise)?;
        Ok(NoiseTransport { state })
    }
}

/// The post-handshake encrypted transport: AEAD-encrypt/decrypt application messages.
pub struct NoiseTransport {
    state: snow::TransportState,
}

impl NoiseTransport {
    /// Encrypt one application message (≤ [`MAX_PLAINTEXT`] bytes).
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, LinkSecError> {
        if plaintext.len() > MAX_PLAINTEXT {
            return Err(LinkSecError::TooLarge(plaintext.len()));
        }
        let mut buf = vec![0u8; plaintext.len() + 16];
        let n = self
            .state
            .write_message(plaintext, &mut buf)
            .map_err(noise)?;
        buf.truncate(n);
        Ok(buf)
    }

    /// Decrypt one message. Fails if the ciphertext is tampered or out of order.
    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, LinkSecError> {
        let mut buf = vec![0u8; ciphertext.len()];
        let n = self
            .state
            .read_message(ciphertext, &mut buf)
            .map_err(noise)?;
        buf.truncate(n);
        Ok(buf)
    }
}

/// Whether a bind address is loopback-only (`127.0.0.0/8`, `::1`, or `localhost`).
pub fn is_loopback_bind(bind_addr: &str) -> bool {
    // Full socket addr (`ip:port`, `[::1]:port`), then bare IP (`::1`, `127.0.0.1`), then host:port.
    if let Ok(sa) = bind_addr.parse::<std::net::SocketAddr>() {
        return sa.ip().is_loopback();
    }
    if let Ok(ip) = bind_addr.parse::<std::net::IpAddr>() {
        return ip.is_loopback();
    }
    let host = match bind_addr.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !h.contains(':') => h,
        _ => bind_addr,
    };
    let host = host.trim_matches(|c| c == '[' || c == ']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
}

/// Whether the control channel must require authentication for a given bind address (REQ-SEC-CTL-02).
///
/// Auth is mandatory on any non-loopback bind, and may also be forced on loopback via config.
pub fn auth_required(bind_addr: &str, configured_require_auth: bool) -> bool {
    configured_require_auth || !is_loopback_bind(bind_addr)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_handshake(
        psk_i: &[u8; PSK_LEN],
        psk_r: &[u8; PSK_LEN],
    ) -> Result<(NoiseTransport, NoiseTransport), LinkSecError> {
        let mut ini = NoiseHandshake::initiator(psk_i)?;
        let mut res = NoiseHandshake::responder(psk_r)?;
        let m1 = ini.write_message()?; // -> e (+ PSK-keyed tag)
        res.read_message(&m1)?; // fails here on PSK mismatch
        let m2 = res.write_message()?; // -> e, ee
        ini.read_message(&m2)?;
        assert!(ini.is_finished() && res.is_finished());
        Ok((ini.into_transport()?, res.into_transport()?))
    }

    #[test]
    fn matching_psk_handshakes_and_round_trips() {
        let psk = [7u8; PSK_LEN];
        let (mut a, mut b) = run_handshake(&psk, &psk).expect("handshake");
        let ct = a.encrypt(b"control-command: PTT on").unwrap();
        assert_eq!(b.decrypt(&ct).unwrap(), b"control-command: PTT on");
        // Reverse direction too.
        let ct2 = b.encrypt(b"ack").unwrap();
        assert_eq!(a.decrypt(&ct2).unwrap(), b"ack");
    }

    #[test]
    fn mismatched_psk_fails_handshake() {
        let a = [1u8; PSK_LEN];
        let b = [2u8; PSK_LEN];
        assert!(
            run_handshake(&a, &b).is_err(),
            "wrong PSK must not authenticate"
        );
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let psk = [9u8; PSK_LEN];
        let (mut a, mut b) = run_handshake(&psk, &psk).unwrap();
        let mut ct = a.encrypt(b"secret").unwrap();
        let n = ct.len();
        ct[n - 1] ^= 0xFF;
        assert!(b.decrypt(&ct).is_err());
    }

    #[test]
    fn oversized_plaintext_is_rejected() {
        let psk = [3u8; PSK_LEN];
        let (mut a, _b) = run_handshake(&psk, &psk).unwrap();
        assert!(matches!(
            a.encrypt(&vec![0u8; MAX_PLAINTEXT + 1]),
            Err(LinkSecError::TooLarge(_))
        ));
    }

    #[test]
    fn loopback_detection_and_gate() {
        for lo in [
            "127.0.0.1",
            "127.0.0.1:9000",
            "::1",
            "[::1]:9001",
            "localhost",
            "127.5.5.5",
        ] {
            assert!(is_loopback_bind(lo), "{lo} should be loopback");
        }
        for non in ["0.0.0.0", "0.0.0.0:9000", "192.168.1.10", "10.0.0.1:9000"] {
            assert!(!is_loopback_bind(non), "{non} should not be loopback");
        }
        // Auth is forced on non-loopback regardless of config; optional on loopback.
        assert!(auth_required("0.0.0.0:9000", false));
        assert!(!auth_required("127.0.0.1:9000", false));
        assert!(auth_required("127.0.0.1:9000", true));
    }
}
