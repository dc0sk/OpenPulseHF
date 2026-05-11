use std::time::Duration;
use std::time::Instant;

use openpulse_ardop::{ArdopConfig, ArdopServer};
use openpulse_audio::loopback::LoopbackBackend;
use openpulse_modem::ModemEngine;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::matrix::{TestCase, TestResult};

/// ARDOP protocol loopback test: spin up a server in loopback mode, send a frame,
/// verify the echo is received.
pub fn run(case: &TestCase) -> TestResult {
    let start = Instant::now();

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let result = rt.block_on(async { run_async(case).await });

    let duration_ms = start.elapsed().as_millis() as u64;
    let bytes_rx = result.as_ref().map(|v| v.len()).unwrap_or(0);
    let effective_bps = if duration_ms > 0 && bytes_rx > 0 {
        Some((bytes_rx as f64 * 8.0) / (duration_ms as f64 / 1000.0))
    } else {
        None
    };
    TestResult {
        case: case.clone(),
        passed: result.is_ok(),
        skipped: false,
        ber: None,
        bytes_rx,
        duration_ms,
        effective_bps,
        note: result.err().map(|e| e.to_string()),
    }
}

async fn run_async(case: &TestCase) -> Result<Vec<u8>, String> {
    let cmd_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| e.to_string())?;
    let data_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| e.to_string())?;
    let cmd_port = cmd_listener.local_addr().unwrap().port();
    let data_port = data_listener.local_addr().unwrap().port();

    let engine = ModemEngine::new(Box::new(LoopbackBackend::default()));
    let config = ArdopConfig {
        bind_addr: "127.0.0.1".into(),
        command_port: cmd_port,
        data_port,
        mode: case.mode.clone(),
        loopback: true,
    };
    let server = ArdopServer::new(engine, config);
    tokio::spawn(async move {
        let _ = server.run_with_listeners(cmd_listener, data_listener).await;
    });
    tokio::time::sleep(Duration::from_millis(20)).await;

    // Connect command port and verify VERSION
    let cmd_stream = TcpStream::connect(("127.0.0.1", cmd_port))
        .await
        .map_err(|e| e.to_string())?;
    let mut cmd_reader = BufReader::new(cmd_stream);
    cmd_reader
        .get_mut()
        .write_all(b"VERSION\r\n")
        .await
        .map_err(|e| e.to_string())?;
    let mut line = String::new();
    timeout(Duration::from_secs(2), cmd_reader.read_line(&mut line))
        .await
        .map_err(|_| "timeout")?
        .map_err(|e| e.to_string())?;
    if !line.contains("OpenPulseHF") {
        return Err(format!("unexpected VERSION response: {line}"));
    }

    // Connect data port: send a frame, expect echo
    let mut data_stream = TcpStream::connect(("127.0.0.1", data_port))
        .await
        .map_err(|e| e.to_string())?;
    let payload: Vec<u8> = (0..case.payload_len.min(255)).map(|i| i as u8).collect();
    send_frame(&mut data_stream, &payload).await?;
    let echoed = recv_frame(&mut data_stream).await?;

    if echoed != payload {
        return Err("echo mismatch".into());
    }
    Ok(echoed)
}

async fn send_frame(stream: &mut TcpStream, data: &[u8]) -> Result<(), String> {
    let len = data.len() as u16;
    stream
        .write_all(&len.to_be_bytes())
        .await
        .map_err(|e| e.to_string())?;
    stream.write_all(data).await.map_err(|e| e.to_string())?;
    stream.flush().await.map_err(|e| e.to_string())
}

async fn recv_frame(stream: &mut TcpStream) -> Result<Vec<u8>, String> {
    use tokio::io::AsyncReadExt;
    let mut len_buf = [0u8; 2];
    timeout(Duration::from_secs(3), stream.read_exact(&mut len_buf))
        .await
        .map_err(|_| "timeout")?
        .map_err(|e| e.to_string())?;
    let len = u16::from_be_bytes(len_buf) as usize;
    let mut payload = vec![0u8; len];
    timeout(Duration::from_secs(3), stream.read_exact(&mut payload))
        .await
        .map_err(|_| "timeout")?
        .map_err(|e| e.to_string())?;
    Ok(payload)
}
