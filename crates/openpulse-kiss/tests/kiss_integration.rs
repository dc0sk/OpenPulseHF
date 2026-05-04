use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

use openpulse_audio::loopback::LoopbackBackend;
use openpulse_kiss::ax25::{Ax25Addr, Ax25UiFrame};
use openpulse_kiss::kiss;
use openpulse_kiss::{KissConfig, KissServer};
use openpulse_modem::ModemEngine;

/// Bind a KISS TNC server on a random port and return its address.
async fn start_server(loopback: bool) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let engine = ModemEngine::new(Box::new(LoopbackBackend::default()));
    let config = KissConfig {
        bind_addr: "127.0.0.1".into(),
        port: addr.port(),
        mode: "BPSK250".into(),
        loopback,
    };
    let server = KissServer::new(engine, config);
    tokio::spawn(async move {
        let _ = server.run_with_listener(listener).await;
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    addr
}

/// Send a complete KISS-encoded frame over TCP.
async fn send_kiss(stream: &mut TcpStream, payload: &[u8]) {
    let frame = kiss::encode(kiss::KISS_DATA, payload);
    stream.write_all(&frame).await.unwrap();
    stream.flush().await.unwrap();
}

/// Receive one KISS frame from TCP; returns the decoded payload.
async fn recv_kiss(stream: &mut TcpStream) -> Vec<u8> {
    let mut buf = Vec::new();
    // Skip to first FEND.
    loop {
        let b = timeout(Duration::from_secs(2), stream.read_u8())
            .await
            .expect("timeout")
            .unwrap();
        if b == 0xC0 {
            break;
        }
    }
    // Accumulate until next FEND.
    loop {
        let b = timeout(Duration::from_secs(2), stream.read_u8())
            .await
            .expect("timeout")
            .unwrap();
        if b == 0xC0 {
            break;
        }
        buf.push(b);
    }
    let (_, payload) = kiss::decode(&buf).expect("decode failed");
    payload
}

// ── Unit tests: KISS codec ────────────────────────────────────────────────────

#[test]
fn kiss_encode_decode_roundtrip() {
    let payload = b"Hello APRS";
    let frame = kiss::encode(kiss::KISS_DATA, payload);
    let (t, decoded) = kiss::decode(&frame[1..frame.len() - 1]).unwrap();
    assert_eq!(t, kiss::KISS_DATA);
    assert_eq!(decoded, payload);
}

#[test]
fn kiss_fend_in_payload_escaped() {
    let payload = vec![0x00, 0xC0, 0xFF]; // 0xC0 = FEND
    let frame = kiss::encode(kiss::KISS_DATA, &payload);
    // Encoded body should contain 0xDB 0xDC instead of 0xC0.
    let body = &frame[1..frame.len() - 1]; // strip outer FENDs
    assert!(!body.contains(&0xC0), "raw FEND must not appear in body");
    let (_, decoded) = kiss::decode(body).unwrap();
    assert_eq!(decoded, payload);
}

#[test]
fn kiss_fesc_in_payload_escaped() {
    let payload = vec![0xDB, 0x01]; // 0xDB = FESC
    let frame = kiss::encode(kiss::KISS_DATA, &payload);
    let body = &frame[1..frame.len() - 1];
    let (_, decoded) = kiss::decode(body).unwrap();
    assert_eq!(decoded, payload);
}

// ── Unit tests: AX.25 codec ───────────────────────────────────────────────────

#[test]
fn ax25_callsign_parse_encode() {
    let addr = Ax25Addr::parse("W1AW-9").unwrap();
    assert_eq!(addr.callsign_str(), "W1AW");
    assert_eq!(addr.ssid, 9);

    let addr2 = Ax25Addr::parse("APRS").unwrap();
    assert_eq!(addr2.callsign_str(), "APRS");
    assert_eq!(addr2.ssid, 0);
}

#[test]
fn ax25_ui_frame_roundtrip() {
    let frame = Ax25UiFrame {
        dest: Ax25Addr::parse("APRS").unwrap(),
        src: Ax25Addr::parse("W1AW-9").unwrap(),
        info: b"!4903.50N/07201.75W-PHG5132".to_vec(),
    };
    let encoded = frame.encode();
    let decoded = Ax25UiFrame::decode(&encoded).unwrap();
    assert_eq!(decoded.dest.callsign_str(), "APRS");
    assert_eq!(decoded.src.callsign_str(), "W1AW");
    assert_eq!(decoded.src.ssid, 9);
    assert_eq!(decoded.info, frame.info);
}

// ── Integration tests: TCP loopback ──────────────────────────────────────────

#[tokio::test]
async fn tcp_single_frame_loopback() {
    let addr = start_server(true).await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    let payload = b"test frame";
    send_kiss(&mut stream, payload).await;

    let received = recv_kiss(&mut stream).await;
    assert_eq!(received, payload);
}

#[tokio::test]
async fn tcp_multi_frame_loopback() {
    let addr = start_server(true).await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    for i in 0u8..4 {
        let payload = vec![i; (i as usize + 1) * 8];
        send_kiss(&mut stream, &payload).await;
        let received = recv_kiss(&mut stream).await;
        assert_eq!(received, payload, "frame {i} mismatch");
    }
}

#[tokio::test]
async fn tcp_byte_stuffed_loopback() {
    let addr = start_server(true).await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Payload contains both FEND (0xC0) and FESC (0xDB) bytes.
    let payload = vec![0x01, 0xC0, 0x02, 0xDB, 0x03];
    send_kiss(&mut stream, &payload).await;

    let received = recv_kiss(&mut stream).await;
    assert_eq!(received, payload);
}
