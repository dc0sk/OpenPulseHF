use std::time::Duration;
use std::time::Instant;

use openpulse_audio::loopback::LoopbackBackend;
use openpulse_kiss::kiss;
use openpulse_kiss::{KissConfig, KissServer};
use openpulse_modem::ModemEngine;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::matrix::{TestCase, TestResult};

/// KISS protocol loopback test: spin up KissServer in loopback mode, send a KISS
/// frame, verify the echo is received correctly.
pub fn run(case: &TestCase) -> TestResult {
    let start = Instant::now();

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let result = rt.block_on(async { run_async(case).await });

    TestResult {
        case: case.clone(),
        passed: result.is_ok(),
        ber: None,
        bytes_rx: result.as_ref().map(|v| v.len()).unwrap_or(0),
        duration_ms: start.elapsed().as_millis() as u64,
        effective_bps: None,
        note: result.err().map(|e| e.to_string()),
    }
}

async fn run_async(case: &TestCase) -> Result<Vec<u8>, String> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| e.to_string())?;
    let addr = listener.local_addr().unwrap();

    let engine = ModemEngine::new(Box::new(LoopbackBackend::default()));
    let config = KissConfig {
        bind_addr: "127.0.0.1".into(),
        port: addr.port(),
        mode: case.mode.clone(),
        loopback: true,
    };
    let server = KissServer::new(engine, config);
    tokio::spawn(async move {
        let _ = server.run_with_listener(listener).await;
    });
    tokio::time::sleep(Duration::from_millis(20)).await;

    let mut stream = TcpStream::connect(addr).await.map_err(|e| e.to_string())?;

    let payload: Vec<u8> = (0..case.payload_len.min(255)).map(|i| i as u8).collect();
    let frame = kiss::encode(kiss::KISS_DATA, &payload);
    stream.write_all(&frame).await.map_err(|e| e.to_string())?;
    stream.flush().await.map_err(|e| e.to_string())?;

    let echoed = recv_kiss(&mut stream).await?;

    if echoed != payload {
        return Err("KISS echo mismatch".into());
    }
    Ok(echoed)
}

/// Receive one KISS-framed payload from TCP.
async fn recv_kiss(stream: &mut TcpStream) -> Result<Vec<u8>, String> {
    // Skip to first FEND (0xC0)
    loop {
        let b = timeout(Duration::from_secs(3), stream.read_u8())
            .await
            .map_err(|_| "timeout waiting for FEND")?
            .map_err(|e| e.to_string())?;
        if b == 0xC0 {
            break;
        }
    }
    // Accumulate until next FEND
    let mut buf = Vec::new();
    loop {
        let b = timeout(Duration::from_secs(3), stream.read_u8())
            .await
            .map_err(|_| "timeout reading KISS frame")?
            .map_err(|e| e.to_string())?;
        if b == 0xC0 {
            break;
        }
        buf.push(b);
    }
    // Decode: buf contains type_byte + escaped_payload (without FENDs)
    if buf.is_empty() {
        return Err("empty KISS frame".into());
    }
    let (_type_byte, payload) = kiss::decode(&buf).map_err(|e| format!("{e:?}"))?;
    Ok(payload)
}
