use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;

use openpulse_ardop::{ArdopConfig, ArdopServer};
use openpulse_audio::loopback::LoopbackBackend;
use openpulse_modem::ModemEngine;

/// Bind the TNC server on random ports and return (cmd_port, data_port).
async fn start_server(loopback: bool) -> (u16, u16) {
    let cmd_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let data_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cmd_port = cmd_listener.local_addr().unwrap().port();
    let data_port = data_listener.local_addr().unwrap().port();

    let engine = ModemEngine::new(Box::new(LoopbackBackend::default()));
    let config = ArdopConfig {
        bind_addr: "127.0.0.1".into(),
        command_port: cmd_port,
        data_port,
        mode: "BPSK250".into(),
        loopback,
    };
    let server = ArdopServer::new(engine, config);
    tokio::spawn(async move {
        let _ = server.run_with_listeners(cmd_listener, data_listener).await;
    });
    // Give the server a moment to start accepting.
    tokio::time::sleep(Duration::from_millis(20)).await;
    (cmd_port, data_port)
}

/// Send a command line and read one response line.
async fn cmd(reader: &mut BufReader<TcpStream>, cmd_str: &str) -> String {
    reader
        .get_mut()
        .write_all(format!("{cmd_str}\r\n").as_bytes())
        .await
        .unwrap();
    reader.get_mut().flush().await.unwrap();
    let mut line = String::new();
    timeout(Duration::from_secs(2), reader.read_line(&mut line))
        .await
        .expect("timeout waiting for response")
        .unwrap();
    line.trim().to_string()
}

/// Read one line from the command socket (for unsolicited pushes).
async fn read_line(reader: &mut BufReader<TcpStream>) -> String {
    let mut line = String::new();
    timeout(Duration::from_secs(2), reader.read_line(&mut line))
        .await
        .expect("timeout waiting for event")
        .unwrap();
    line.trim().to_string()
}

/// Send a u16-BE framed message over the data port.
async fn send_data(stream: &mut TcpStream, payload: &[u8]) {
    let len = payload.len() as u16;
    stream.write_all(&len.to_be_bytes()).await.unwrap();
    stream.write_all(payload).await.unwrap();
    stream.flush().await.unwrap();
}

/// Receive a u16-BE framed message from the data port.
async fn recv_data(stream: &mut TcpStream) -> Vec<u8> {
    let mut len_buf = [0u8; 2];
    timeout(Duration::from_secs(2), stream.read_exact(&mut len_buf))
        .await
        .expect("timeout waiting for data frame length")
        .unwrap();
    let len = u16::from_be_bytes(len_buf) as usize;
    let mut payload = vec![0u8; len];
    timeout(Duration::from_secs(2), stream.read_exact(&mut payload))
        .await
        .expect("timeout waiting for data frame payload")
        .unwrap();
    payload
}

#[tokio::test]
async fn version_response() {
    let (cmd_port, _) = start_server(false).await;
    let stream = TcpStream::connect(("127.0.0.1", cmd_port)).await.unwrap();
    let mut reader = BufReader::new(stream);
    let resp = cmd(&mut reader, "VERSION").await;
    assert_eq!(resp, "VERSION 1.0-OpenPulseHF");
}

#[tokio::test]
async fn myid_echo() {
    let (cmd_port, _) = start_server(false).await;
    let stream = TcpStream::connect(("127.0.0.1", cmd_port)).await.unwrap();
    let mut reader = BufReader::new(stream);
    let resp = cmd(&mut reader, "MYID W1AW").await;
    assert_eq!(resp, "MYID W1AW");
}

#[tokio::test]
async fn state_disc_initially() {
    let (cmd_port, _) = start_server(false).await;
    let stream = TcpStream::connect(("127.0.0.1", cmd_port)).await.unwrap();
    let mut reader = BufReader::new(stream);
    let resp = cmd(&mut reader, "STATE").await;
    assert_eq!(resp, "STATE DISC");
}

#[tokio::test]
async fn connect_and_disconnect() {
    let (cmd_port, _) = start_server(false).await;
    let stream = TcpStream::connect(("127.0.0.1", cmd_port)).await.unwrap();
    let mut reader = BufReader::new(stream);

    // CONNECT returns NEWSTATE CONNECTING then CONNECTED <peer>.
    reader
        .get_mut()
        .write_all(b"CONNECT 500 W1XYZ\r\n")
        .await
        .unwrap();
    reader.get_mut().flush().await.unwrap();
    let line1 = read_line(&mut reader).await;
    let line2 = read_line(&mut reader).await;
    assert_eq!(line1, "NEWSTATE CONNECTING");
    assert_eq!(line2, "CONNECTED W1XYZ");

    // Verify connected state.
    let state_resp = cmd(&mut reader, "STATE").await;
    assert!(
        state_resp.contains("CONNECTED"),
        "unexpected state: {state_resp}"
    );

    // DISCONNECT returns NEWSTATE DISCONNECTING then DISCONNECTED.
    reader.get_mut().write_all(b"DISCONNECT\r\n").await.unwrap();
    reader.get_mut().flush().await.unwrap();
    let ev1 = read_line(&mut reader).await;
    let ev2 = read_line(&mut reader).await;
    assert_eq!(ev1, "NEWSTATE DISCONNECTING");
    assert_eq!(ev2, "DISCONNECTED");
}

#[tokio::test]
async fn abort_resets_to_disc() {
    let (cmd_port, _) = start_server(false).await;
    let stream = TcpStream::connect(("127.0.0.1", cmd_port)).await.unwrap();
    let mut reader = BufReader::new(stream);
    let resp = cmd(&mut reader, "ABORT").await;
    assert_eq!(resp, "NEWSTATE DISC");
}

#[tokio::test]
async fn buffer_reports_zero_initially() {
    let (cmd_port, _) = start_server(false).await;
    let stream = TcpStream::connect(("127.0.0.1", cmd_port)).await.unwrap();
    let mut reader = BufReader::new(stream);
    let resp = cmd(&mut reader, "BUFFER").await;
    assert_eq!(resp, "BUFFER 0");
}

#[tokio::test]
async fn data_port_loopback_roundtrip() {
    let (_, data_port) = start_server(true).await;
    let mut stream = TcpStream::connect(("127.0.0.1", data_port)).await.unwrap();

    let payload = b"Hello ARDOP";
    send_data(&mut stream, payload).await;

    let received = recv_data(&mut stream).await;
    assert_eq!(received, payload);
}

#[tokio::test]
async fn data_port_multiple_frames() {
    let (_, data_port) = start_server(true).await;
    let mut stream = TcpStream::connect(("127.0.0.1", data_port)).await.unwrap();

    for i in 0u8..4 {
        let payload = vec![i; (i as usize + 1) * 10];
        send_data(&mut stream, &payload).await;
        let received = recv_data(&mut stream).await;
        assert_eq!(received, payload, "frame {i} mismatch");
    }
}

#[tokio::test]
async fn gridsquare_get_set() {
    let (cmd_port, _) = start_server(false).await;
    let stream = TcpStream::connect(("127.0.0.1", cmd_port)).await.unwrap();
    let mut reader = BufReader::new(stream);

    let resp = cmd(&mut reader, "GRIDSQUARE FN42").await;
    assert_eq!(resp, "GRIDSQUARE FN42");

    let resp2 = cmd(&mut reader, "GRIDSQUARE").await;
    assert_eq!(resp2, "GRIDSQUARE FN42");
}

#[tokio::test]
async fn arqbw_get_set() {
    let (cmd_port, _) = start_server(false).await;
    let stream = TcpStream::connect(("127.0.0.1", cmd_port)).await.unwrap();
    let mut reader = BufReader::new(stream);

    let resp = cmd(&mut reader, "ARQBW 1000").await;
    assert_eq!(resp, "ARQBW 1000");

    let resp2 = cmd(&mut reader, "ARQBW").await;
    assert_eq!(resp2, "ARQBW 1000");
}

#[tokio::test]
async fn ping_pong() {
    let (cmd_port, _) = start_server(false).await;
    let stream = TcpStream::connect(("127.0.0.1", cmd_port)).await.unwrap();
    let mut reader = BufReader::new(stream);
    let resp = cmd(&mut reader, "PING").await;
    assert_eq!(resp, "PONG");
}
