//! Direct TCP Winlink CMS gateway.
//!
//! Connects to a Winlink CMS at `cms.winlink.org:8772` (or any B2F TCP
//! gateway), sends queued outbound messages, and receives any messages
//! the CMS has waiting for the station callsign — all in a single session.

use std::io::{self, Read};
use std::net::TcpStream;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use clap::{Parser, Subcommand};

use openpulse_b2f::{B2fSession, SessionRole, WlHeader};
use openpulse_b2f_driver::{DataPort, DriverError};

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "openpulse-gateway",
    about = "Direct TCP connection to a Winlink CMS gateway",
    long_about = "Direct TCP connection to a Winlink CMS gateway.",
    author,
    version
)]
struct Cli {
    /// CMS hostname or IP address.
    #[arg(long, default_value = "cms.winlink.org")]
    host: String,

    /// CMS TCP port.
    #[arg(long, default_value_t = 8772)]
    port: u16,

    /// Station callsign (overrides config file).
    #[arg(long)]
    callsign: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Send a message and receive any pending messages from the CMS.
    Send {
        /// Recipient callsign.
        #[arg(long)]
        to: String,

        /// Message subject line.
        #[arg(long, default_value = "OpenPulse test message")]
        subject: String,

        /// Message body. Reads from stdin if omitted.
        #[arg(long)]
        message: Option<String>,
    },
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Generate a Winlink-style message ID from callsign + current Unix timestamp.
fn generate_mid(callsign: &str) -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as u32;
    let call = callsign
        .to_uppercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .take(6)
        .collect::<String>();
    format!("{call}{secs:08X}.1.")
}

fn is_leap(y: u32) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

/// Current UTC date/time as `"YYYY/MM/DD HH:MM"`.
fn now_date_str() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let s = secs % 86400;
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let mut days = (secs / 86400) as u32;
    let mut year = 1970u32;
    loop {
        let y_days = if is_leap(year) { 366 } else { 365 };
        if days < y_days {
            break;
        }
        days -= y_days;
        year += 1;
    }
    let month_days = [
        31u32,
        if is_leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u32;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    let day = days + 1;
    format!("{year:04}/{month:02}/{day:02} {h:02}:{m:02}")
}

/// Build a plain-text `WlHeader` for an outbound message.
fn build_header(from: &str, to: &str, subject: &str, body_len: u32) -> WlHeader {
    WlHeader {
        mid: generate_mid(from),
        date: now_date_str(),
        from: format!("{}@winlink.org", from.to_uppercase()),
        to: vec![format!("{}@winlink.org", to.to_uppercase())],
        subject: subject.to_string(),
        size: body_len,
        body: body_len,
        attachments: vec![],
    }
}

// ── Session logic ─────────────────────────────────────────────────────────────

/// Phase 1 (ISS): send `messages` to the CMS.
///
/// Reads the CMS banner, sends FC proposals + FF, reads FS, sends blobs.
pub(crate) fn iss_send(
    data: &mut DataPort,
    messages: Vec<(WlHeader, Vec<u8>)>,
) -> anyhow::Result<()> {
    let msg_count = messages.len();
    let mut session = B2fSession::new(SessionRole::Iss);
    for (h, b) in messages {
        session.queue_message(h, b)?;
    }

    let frame = data.recv_frame().context("reading CMS banner")?;
    let line = String::from_utf8_lossy(&frame).into_owned();
    tracing::info!("← {}", line.trim());

    let responses = session.handle_line(&line)?;
    for resp in &responses {
        tracing::debug!("→ {}", resp.trim());
        data.send_frame(resp.as_bytes())?;
    }

    let frame = data.recv_frame().context("reading CMS FS")?;
    let line = String::from_utf8_lossy(&frame).into_owned();
    tracing::debug!("← {}", line.trim());
    session.handle_line(&line)?;

    let blobs = session.drain_pending_data();
    if msg_count > 0 && blobs.is_empty() {
        anyhow::bail!("CMS rejected all {} proposed message(s)", msg_count);
    }
    for blob in blobs {
        tracing::debug!("→ [blob {} B]", blob.len());
        data.send_frame(&blob)?;
    }

    Ok(())
}

/// Phase 2 (IRS): receive any messages the CMS proposes back.
///
/// Uses a fresh `B2fSession(Irs)` on the same `DataPort`. Returns the
/// decompressed body bytes for each message. Returns an empty `Vec` if the
/// CMS has no pending messages (FQ or TCP close).
pub(crate) fn irs_receive(data: &mut DataPort) -> anyhow::Result<Vec<Vec<u8>>> {
    let mut session = B2fSession::new(SessionRole::Irs);

    loop {
        match data.recv_frame() {
            Ok(frame) => {
                let line = String::from_utf8_lossy(&frame).into_owned();
                tracing::debug!("← {}", line.trim());
                let responses = session.handle_line(&line)?;
                for resp in &responses {
                    tracing::debug!("→ {}", resp.trim());
                    data.send_frame(resp.as_bytes())?;
                }
                if !responses.is_empty() || session.is_done() {
                    break;
                }
            }
            Err(DriverError::Io(e))
                if matches!(
                    e.kind(),
                    io::ErrorKind::UnexpectedEof
                        | io::ErrorKind::ConnectionReset
                        | io::ErrorKind::BrokenPipe
                ) =>
            {
                break;
            }
            Err(e) => return Err(e.into()),
        }
    }

    let count = session.accepted_count();
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let blob = data.recv_frame().context("reading CMS message blob")?;
        out.push(session.receive_data(blob)?);
    }
    Ok(out)
}

/// Open a TCP connection to `host:port`, run the full ISS+IRS exchange, and
/// return decompressed body bytes for each message received from the CMS.
fn connect_and_exchange(
    host: &str,
    port: u16,
    messages: Vec<(WlHeader, Vec<u8>)>,
) -> anyhow::Result<Vec<Vec<u8>>> {
    // Winlink CMS port 8772 uses the B2F wire protocol, which is unauthenticated
    // plaintext by specification. TLS is not available at this port.
    tracing::warn!(
        host,
        port,
        "connecting to Winlink CMS over unauthenticated plaintext (B2F protocol)"
    );
    let stream =
        TcpStream::connect((host, port)).with_context(|| format!("connecting to {host}:{port}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    let mut data = DataPort::new(stream);
    iss_send(&mut data, messages)?;
    irs_receive(&mut data)
}

// ── main ──────────────────────────────────────────────────────────────────────

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let cfg = openpulse_config::load()?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&cfg.logging.level)),
        )
        .init();

    let callsign = cli
        .callsign
        .as_deref()
        .unwrap_or(&cfg.station.callsign)
        .to_string();

    if callsign == "N0CALL" {
        anyhow::bail!(
            "no callsign configured — set [station] callsign in config or pass --callsign"
        );
    }

    match cli.command {
        Commands::Send {
            to,
            subject,
            message,
        } => {
            let body: Vec<u8> = if let Some(text) = message {
                text.into_bytes()
            } else {
                let mut buf = Vec::new();
                io::stdin()
                    .read_to_end(&mut buf)
                    .context("reading message body from stdin")?;
                buf
            };

            let header = build_header(&callsign, &to, &subject, body.len() as u32);
            tracing::info!("Connecting to {}:{} as {}", cli.host, cli.port, callsign);

            let received = connect_and_exchange(&cli.host, cli.port, vec![(header, body)])?;

            if received.is_empty() {
                println!("No pending messages.");
            } else {
                println!("--- {} message(s) received ---", received.len());
                for (i, msg) in received.iter().enumerate() {
                    println!("=== Message {} ===", i + 1);
                    println!("{}", String::from_utf8_lossy(msg));
                }
            }
        }
    }

    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    use openpulse_b2f::{banner, compress_gzip, frame, header};

    fn tcp_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (server, _) = listener.accept().unwrap();
        (client, server)
    }

    /// Full round-trip: gateway sends one message, mock CMS sends one reply.
    /// A hostile/broken CMS must not be able to hang or mislead the gateway. The cooperative mock
    /// in `gateway_round_trip` cannot show any of this (audit 2026-07-17, low tier).
    #[test]
    fn iss_send_reports_a_cms_that_rejects_everything() {
        let (client_stream, server_stream) = tcp_pair();
        let server = thread::spawn(move || {
            let mut cms = DataPort::new(server_stream);
            cms.send_frame(banner::encode("W1AW").as_bytes()).unwrap();
            let _fc = cms.recv_frame().unwrap();
            let _ff = cms.recv_frame().unwrap();
            let fs = frame::encode(&openpulse_b2f::frame::B2fFrame::Fs {
                answers: vec![openpulse_b2f::frame::FsAnswer::Reject],
            });
            cms.send_frame(fs.as_bytes()).unwrap();
        });

        let mut data = DataPort::new(client_stream);
        let msgs = vec![(
            build_header("K1TEST@winlink.org", "W1AW@winlink.org", "Test", 5),
            b"hello".to_vec(),
        )];
        let got = iss_send(&mut data, msgs);
        assert!(
            got.is_err(),
            "a CMS that rejects every proposal must be an error, got {got:?}"
        );
        server.join().unwrap();
    }

    /// Garbage where the banner belongs must be a clean error, not a panic or a hang.
    #[test]
    fn iss_send_rejects_a_garbage_banner() {
        let (client_stream, server_stream) = tcp_pair();
        let server = thread::spawn(move || {
            let mut cms = DataPort::new(server_stream);
            cms.send_frame(b"this is not a WL2K banner").unwrap();
            thread::sleep(std::time::Duration::from_millis(100));
        });

        let mut data = DataPort::new(client_stream);
        let msgs = vec![(
            build_header("K1TEST@winlink.org", "W1AW@winlink.org", "Test", 5),
            b"hello".to_vec(),
        )];
        assert!(iss_send(&mut data, msgs).is_err());
        server.join().unwrap();
    }

    /// A CMS that proposes a message and then never sends its blob must time out rather than block
    /// the gateway forever.
    #[test]
    fn irs_receive_times_out_on_a_promised_blob_that_never_arrives() {
        let (client_stream, server_stream) = tcp_pair();
        let server = thread::spawn(move || {
            let mut cms = DataPort::new(server_stream);
            let fc = frame::encode(&openpulse_b2f::frame::B2fFrame::Fc {
                proposal_type: openpulse_b2f::frame::ProposalType::D,
                mid: "W1AW00000001.1.".into(),
                size: 64,
                date: "2026/01/01 00:00".into(),
            });
            cms.send_frame(fc.as_bytes()).unwrap();
            cms.send_frame(b"FF\r").unwrap();
            let _fs = cms.recv_frame();
            // Promised a blob; never sends it, and holds the connection open so the gateway cannot
            // mistake a close for an answer — only a real timeout can end this.
            thread::sleep(std::time::Duration::from_secs(10));
        });

        let mut data = DataPort::new(client_stream);
        data.set_timeout(Some(std::time::Duration::from_millis(400)))
            .unwrap();
        let started = std::time::Instant::now();
        let got = irs_receive(&mut data);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "must fail on the timeout, not by waiting for the peer to go away"
        );
        assert!(
            got.is_err(),
            "a promised-but-unsent blob must time out, got {got:?}"
        );
        // Deliberately not joined: the peer holds the connection for 10 s, and the point of the test
        // is that we returned long before it let go.
        drop(server);
    }

    #[test]
    fn gateway_round_trip() {
        let (client_stream, server_stream) = tcp_pair();

        let outbound_body = b"Hello from gateway".to_vec();
        let reply_body = b"Reply from CMS".to_vec();
        let expected_reply = reply_body.clone();
        let expected_outbound = outbound_body.clone();

        let server_thread = thread::spawn(move || {
            let mut cms = DataPort::new(server_stream);
            let mut irs = B2fSession::new(SessionRole::Irs);

            // ── Phase 1: receive the gateway's outbound message ──

            cms.send_frame(banner::encode("W1AW").as_bytes()).unwrap();

            // Read FC+FF proposals, respond with FS.
            loop {
                let raw = cms.recv_frame().unwrap();
                let line = String::from_utf8_lossy(&raw).into_owned();
                let responses = irs.handle_line(&line).unwrap();
                for resp in &responses {
                    cms.send_frame(resp.as_bytes()).unwrap();
                }
                if !responses.is_empty() || irs.is_done() {
                    break;
                }
            }

            // Read the compressed blob.
            let count = irs.accepted_count();
            let mut received_outbound = Vec::new();
            for _ in 0..count {
                let blob = cms.recv_frame().unwrap();
                received_outbound.push(irs.receive_data(blob).unwrap());
            }
            // Decompress blob and split into header + body.
            assert_eq!(received_outbound.len(), 1);
            let raw = &received_outbound[0];
            let sep_pos = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
            let hdr = header::decode(&raw[..sep_pos]).unwrap();
            let body = &raw[sep_pos + 4..];
            assert_eq!(hdr.to, vec!["W1AW@winlink.org"]);
            assert_eq!(hdr.from, "K1TEST@winlink.org");
            assert_eq!(hdr.subject, "Test");
            assert_eq!(body, expected_outbound.as_slice());

            // ── Phase 2: propose a reply to the gateway ──

            // Compress the reply body and send FC+FF directly (gateway acts as IRS).
            let reply_compressed = compress_gzip(&reply_body).unwrap();
            let fc = frame::encode(&openpulse_b2f::frame::B2fFrame::Fc {
                proposal_type: openpulse_b2f::frame::ProposalType::D,
                mid: "W1AW00000001.1.".into(),
                size: reply_compressed.len() as u32,
                date: "2026/01/01 00:00".into(),
            });
            cms.send_frame(fc.as_bytes()).unwrap();
            cms.send_frame(b"FF\r").unwrap();

            // Read FS from gateway (it should accept).
            let fs_raw = cms.recv_frame().unwrap();
            let fs_line = String::from_utf8_lossy(&fs_raw).into_owned();
            assert!(
                fs_line.trim_start().starts_with("FS"),
                "expected FS, got: {fs_line}"
            );

            // Send the compressed reply blob.
            cms.send_frame(&reply_compressed).unwrap();
        });

        // Gateway side.
        let header = build_header("K1TEST", "W1AW", "Test", outbound_body.len() as u32);
        let mut data = DataPort::new(client_stream);
        iss_send(&mut data, vec![(header, outbound_body)]).unwrap();
        let received = irs_receive(&mut data).unwrap();

        server_thread.join().unwrap();

        assert_eq!(received.len(), 1);
        assert_eq!(received[0], expected_reply);
    }
}
