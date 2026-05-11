//! Integration tests for the binary spectrum channel (FF-6).

use std::net::SocketAddr;
use std::time::Duration;

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_daemon::protocol::{
    decode_spectrum_frame, encode_spectrum_frame, ControlCommand, SPECTRUM_MAGIC,
};
use openpulse_daemon::{ControlServer, ControlServerHandle};
use openpulse_modem::ModemEngine;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;

fn make_engine() -> ModemEngine {
    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
    engine.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    engine
}

async fn spawn_server(engine: &ModemEngine) -> (SocketAddr, ControlServerHandle) {
    let mut addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let handle = ControlServer::spawn(
        "127.0.0.1:0".parse().unwrap(),
        engine,
        "BPSK250".into(),
        ("N0CALL".into(), "AA00".into()), // station_id
        Some(&mut addr),
    )
    .await
    .unwrap();
    (addr, handle)
}

// ---------------------------------------------------------------------------
// Unit test: codec round-trip
// ---------------------------------------------------------------------------

#[test]
fn encode_decode_spectrum_round_trip() {
    let bins: Vec<f32> = (0..512).map(|i| -(i as f32) * 0.25).collect();
    let frame = encode_spectrum_frame(8000, &bins);

    // Magic header present.
    assert_eq!(&frame[0..4], SPECTRUM_MAGIC);

    let (sample_rate, decoded) = decode_spectrum_frame(&frame).unwrap();
    assert_eq!(sample_rate, 8000);
    assert_eq!(decoded.len(), 512);
    for (a, b) in bins.iter().zip(decoded.iter()) {
        assert!((a - b).abs() < 1e-6, "bin mismatch: {a} vs {b}");
    }
}

#[test]
fn decode_spectrum_frame_rejects_bad_magic() {
    let mut frame = encode_spectrum_frame(8000, &[0.0f32; 16]);
    frame[0] = 0x00; // corrupt magic
    assert!(decode_spectrum_frame(&frame).is_err());
}

#[test]
fn decode_spectrum_frame_rejects_truncated() {
    let frame = encode_spectrum_frame(8000, &[0.0f32; 16]);
    assert!(decode_spectrum_frame(&frame[..9]).is_err());
}

// ---------------------------------------------------------------------------
// Integration tests: spectrum subscription
// ---------------------------------------------------------------------------

/// Read bytes from a tokio stream, peeking at the first byte to decide whether
/// to read a binary spectrum frame or an NDJSON line.
async fn read_next_message(
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
) -> Option<Either> {
    let buf = reader.fill_buf().await.ok()?;
    if buf.is_empty() {
        return None;
    }
    let first = buf[0];

    if first == SPECTRUM_MAGIC[0] {
        let mut header = [0u8; 10];
        reader.read_exact(&mut header).await.ok()?;
        if &header[0..4] != SPECTRUM_MAGIC {
            return None;
        }
        let fft_size = u16::from_le_bytes([header[4], header[5]]) as usize;
        let mut bin_bytes = vec![0u8; fft_size * 4];
        reader.read_exact(&mut bin_bytes).await.ok()?;
        Some(Either::Binary { fft_size })
    } else {
        let mut line = String::new();
        reader.read_line(&mut line).await.ok()?;
        Some(Either::Text(line))
    }
}

#[allow(dead_code)]
enum Either {
    Binary { fft_size: usize },
    Text(String),
}

#[tokio::test]
async fn subscribe_spectrum_sends_binary_frames() {
    let engine = make_engine();
    let (addr, _handle) = spawn_server(&engine).await;

    let stream = TcpStream::connect(addr).await.unwrap();
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Subscribe at 50 fps.
    let cmd = serde_json::to_string(&ControlCommand::SubscribeSpectrum { fps: 50 }).unwrap();
    write_half
        .write_all(format!("{cmd}\n").as_bytes())
        .await
        .unwrap();

    // Collect messages for 150 ms; count binary spectrum frames.
    let mut binary_count = 0usize;
    let deadline = Duration::from_millis(150);
    let _ = timeout(deadline, async {
        loop {
            match read_next_message(&mut reader).await {
                Some(Either::Binary { fft_size }) => {
                    assert_eq!(fft_size, 512, "expected 512 bins");
                    binary_count += 1;
                    if binary_count >= 5 {
                        break;
                    }
                }
                Some(Either::Text(_)) => {} // CommandResponse or Metrics — fine
                None => break,
            }
        }
    })
    .await;

    assert!(
        binary_count >= 5,
        "expected ≥5 binary frames in 150 ms at 50 fps, got {binary_count}"
    );
}

#[tokio::test]
async fn ndjson_events_unaffected_by_spectrum_subscription() {
    let engine = make_engine();
    let (addr, _handle) = spawn_server(&engine).await;

    // Client A: subscribe to spectrum.
    let stream_a = TcpStream::connect(addr).await.unwrap();
    let (read_a, mut write_a) = stream_a.into_split();
    let mut reader_a = BufReader::new(read_a);
    let cmd = serde_json::to_string(&ControlCommand::SubscribeSpectrum { fps: 20 }).unwrap();
    write_a
        .write_all(format!("{cmd}\n").as_bytes())
        .await
        .unwrap();

    // Client B: no spectrum subscription.
    let stream_b = TcpStream::connect(addr).await.unwrap();
    let (read_b, _write_b) = stream_b.into_split();
    let mut reader_b = BufReader::new(read_b);

    // Wait up to 1.5 s for a Metrics NDJSON event on client B.
    let got_metrics_b = timeout(Duration::from_millis(1500), async {
        loop {
            let mut line = String::new();
            if reader_b.read_line(&mut line).await.ok() == Some(0) {
                break false;
            }
            if line.contains("metrics") {
                break true;
            }
        }
    })
    .await
    .unwrap_or(false);

    assert!(
        got_metrics_b,
        "client B should still receive Metrics NDJSON events"
    );

    // Also verify client A receives at least one binary frame.
    let got_binary_a = timeout(Duration::from_millis(200), async {
        loop {
            match read_next_message(&mut reader_a).await {
                Some(Either::Binary { .. }) => break true,
                Some(Either::Text(_)) => {}
                None => break false,
            }
        }
    })
    .await
    .unwrap_or(false);

    assert!(
        got_binary_a,
        "client A should receive binary spectrum frames"
    );
}
