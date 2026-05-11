//! Integration tests for the NDJSON-over-TCP control port (Phase 7.3).

use std::net::SocketAddr;
use std::time::Duration;

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_daemon::protocol::{CommandResponse, ControlCommand, ControlEvent, DaemonConfig};
use openpulse_daemon::{ControlServer, ControlServerHandle};
use openpulse_modem::ModemEngine;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
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

/// Connect a raw TCP client; returns the split read/write halves as a BufReader + write half.
async fn connect(
    addr: SocketAddr,
) -> (
    BufReader<tokio::net::tcp::OwnedReadHalf>,
    tokio::net::tcp::OwnedWriteHalf,
) {
    let stream = TcpStream::connect(addr).await.unwrap();
    let (r, w) = stream.into_split();
    (BufReader::new(r), w)
}

#[tokio::test]
async fn connect_receives_metrics_event() {
    let engine = make_engine();
    let (addr, _handle) = spawn_server(&engine).await;

    let (mut reader, _w) = connect(addr).await;

    // The server emits Metrics at 1 Hz; wait up to 2 seconds.
    let line = timeout(Duration::from_secs(2), async {
        let mut buf = String::new();
        reader.read_line(&mut buf).await.unwrap();
        buf
    })
    .await
    .expect("timed out waiting for first event");

    let ev: ControlEvent = serde_json::from_str(line.trim()).expect("invalid JSON from server");
    assert!(
        matches!(ev, ControlEvent::Metrics { .. }),
        "expected Metrics event, got {ev:?}"
    );
}

#[tokio::test]
async fn engine_event_forwarded_to_client() {
    let mut engine = make_engine();
    let (addr, _handle) = spawn_server(&engine).await;

    let (mut reader, _w) = connect(addr).await;

    // Give the connection a moment to be accepted.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Trigger a FrameTransmitted event.
    engine.transmit(b"test", "BPSK250", None).unwrap();

    // Read lines until we see a FrameTransmitted EngineEvent or timeout.
    let found = timeout(Duration::from_secs(2), async {
        loop {
            let mut buf = String::new();
            reader.read_line(&mut buf).await.unwrap();
            if buf.trim().is_empty() {
                continue;
            }
            let ev: ControlEvent = serde_json::from_str(buf.trim()).expect("invalid JSON");
            if let ControlEvent::EngineEvent {
                event: openpulse_modem::EngineEvent::FrameTransmitted { .. },
            } = ev
            {
                return true;
            }
        }
    })
    .await;

    assert!(
        found.is_ok(),
        "timed out before FrameTransmitted event arrived"
    );
}

#[tokio::test]
async fn set_mode_command_returns_ok() {
    let engine = make_engine();
    let (addr, _handle) = spawn_server(&engine).await;

    let (mut reader, mut writer) = connect(addr).await;

    // Wait for at least one event so we know the connection is live.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let cmd = serde_json::to_string(&ControlCommand::SetMode {
        mode: "BPSK100".into(),
    })
    .unwrap()
        + "\n";
    writer.write_all(cmd.as_bytes()).await.unwrap();

    // Read lines until we see the CommandResponse (not an event line).
    let resp = timeout(Duration::from_secs(2), async {
        loop {
            let mut buf = String::new();
            reader.read_line(&mut buf).await.unwrap();
            if buf.trim().is_empty() {
                continue;
            }
            // CommandResponse has `ok` field; ControlEvent has `type` field.
            if buf.contains("\"ok\"") {
                return serde_json::from_str::<CommandResponse>(buf.trim()).unwrap();
            }
        }
    })
    .await
    .expect("timed out waiting for command response");

    assert!(resp.ok, "set_mode returned error: {:?}", resp.error);
}

#[tokio::test]
async fn invalid_command_returns_error() {
    let engine = make_engine();
    let (addr, _handle) = spawn_server(&engine).await;

    let (mut reader, mut writer) = connect(addr).await;

    tokio::time::sleep(Duration::from_millis(50)).await;

    writer
        .write_all(b"{\"cmd\": \"no_such_command\"}\n")
        .await
        .unwrap();

    let resp = timeout(Duration::from_secs(2), async {
        loop {
            let mut buf = String::new();
            reader.read_line(&mut buf).await.unwrap();
            if buf.contains("\"ok\"") {
                return serde_json::from_str::<CommandResponse>(buf.trim()).unwrap();
            }
        }
    })
    .await
    .expect("timed out waiting for error response");

    assert!(!resp.ok);
    assert!(resp.error.is_some());
}

#[tokio::test]
async fn multiple_clients_both_receive_events() {
    let mut engine = make_engine();
    let (addr, _handle) = spawn_server(&engine).await;

    let (r1, _w1) = connect(addr).await;
    let (r2, _w2) = connect(addr).await;

    tokio::time::sleep(Duration::from_millis(50)).await;

    engine.transmit(b"broadcast", "BPSK250", None).unwrap();

    let check = |mut r: BufReader<tokio::net::tcp::OwnedReadHalf>| async move {
        timeout(Duration::from_secs(2), async move {
            loop {
                let mut buf = String::new();
                r.read_line(&mut buf).await.unwrap();
                if buf.trim().is_empty() {
                    continue;
                }
                let ev: ControlEvent = serde_json::from_str(buf.trim()).expect("bad JSON");
                if let ControlEvent::EngineEvent {
                    event: openpulse_modem::EngineEvent::FrameTransmitted { .. },
                } = ev
                {
                    return true;
                }
            }
        })
        .await
        .is_ok()
    };

    let (ok1, ok2) = tokio::join!(check(r1), check(r2));
    assert!(ok1, "client 1 did not receive FrameTransmitted");
    assert!(ok2, "client 2 did not receive FrameTransmitted");
}

#[tokio::test]
async fn set_tx_attenuation_command_returns_ok() {
    let engine = make_engine();
    let (addr, _handle) = spawn_server(&engine).await;

    let (mut reader, mut writer) = connect(addr).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let cmd = serde_json::to_string(&ControlCommand::SetTxAttenuation {
        db: -6.0,
        band: Some("40m".into()),
    })
    .unwrap()
        + "\n";
    writer.write_all(cmd.as_bytes()).await.unwrap();

    let resp = timeout(Duration::from_secs(2), async {
        loop {
            let mut buf = String::new();
            reader.read_line(&mut buf).await.unwrap();
            if buf.contains("\"ok\"") {
                return serde_json::from_str::<CommandResponse>(buf.trim()).unwrap();
            }
        }
    })
    .await
    .expect("timed out waiting for attenuation response");

    assert!(
        resp.ok,
        "set_tx_attenuation returned error: {:?}",
        resp.error
    );
}

#[tokio::test]
async fn set_tx_attenuation_updates_shared_state() {
    let engine = make_engine();
    let mut addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let handle = ControlServer::spawn(
        "127.0.0.1:0".parse().unwrap(),
        &engine,
        "BPSK250".into(),
        ("N0CALL".into(), "AA00".into()), // station_id
        Some(&mut addr),
    )
    .await
    .unwrap();

    let (mut reader, mut writer) = connect(addr).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let cmd = serde_json::to_string(&ControlCommand::SetTxAttenuation {
        db: -12.5,
        band: None,
    })
    .unwrap()
        + "\n";
    writer.write_all(cmd.as_bytes()).await.unwrap();

    timeout(Duration::from_secs(2), async {
        loop {
            let mut buf = String::new();
            reader.read_line(&mut buf).await.unwrap();
            if buf.contains("\"ok\"") {
                return;
            }
        }
    })
    .await
    .expect("timed out waiting for response");

    let stored = *handle.tx_attenuation_db.lock().await;
    assert!(
        (stored - (-12.5)).abs() < 1e-4,
        "expected -12.5 dB, got {stored}"
    );
}

#[tokio::test]
async fn get_config_returns_config_data_and_ok() {
    let engine = make_engine();
    let mut addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let _handle = ControlServer::spawn(
        "127.0.0.1:0".parse().unwrap(),
        &engine,
        "BPSK250".into(),
        ("K1ABC".into(), "FN42".into()), // station_id
        Some(&mut addr),
    )
    .await
    .unwrap();

    let (mut reader, mut writer) = connect(addr).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let cmd = serde_json::to_string(&ControlCommand::GetConfig).unwrap() + "\n";
    writer.write_all(cmd.as_bytes()).await.unwrap();

    let (config_ev, ok_resp) = timeout(Duration::from_secs(2), async {
        let mut config_ev: Option<ControlEvent> = None;
        let mut ok_resp: Option<CommandResponse> = None;
        loop {
            let mut buf = String::new();
            reader.read_line(&mut buf).await.unwrap();
            let line = buf.trim();
            if line.is_empty() {
                continue;
            }
            if line.contains("\"config_data\"") {
                config_ev = Some(serde_json::from_str(line).unwrap());
            } else if line.contains("\"ok\"") {
                ok_resp = Some(serde_json::from_str(line).unwrap());
            }
            if config_ev.is_some() && ok_resp.is_some() {
                return (config_ev.unwrap(), ok_resp.unwrap());
            }
        }
    })
    .await
    .expect("timed out waiting for GetConfig response");

    assert!(ok_resp.ok, "GetConfig ok response was not ok");
    match config_ev {
        ControlEvent::ConfigData { config } => {
            assert_eq!(config.callsign, "K1ABC");
            assert_eq!(config.grid_square, "FN42");
            assert_eq!(config.mode, "BPSK250");
            assert!((config.tx_attenuation_db - 0.0).abs() < 1e-4);
        }
        other => panic!("expected ConfigData event, got {other:?}"),
    }
}

#[tokio::test]
async fn set_config_updates_mode_and_attenuation_atomically() {
    let engine = make_engine();
    let mut addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let handle = ControlServer::spawn(
        "127.0.0.1:0".parse().unwrap(),
        &engine,
        "BPSK250".into(),
        ("N0CALL".into(), "AA00".into()), // station_id
        Some(&mut addr),
    )
    .await
    .unwrap();

    let (mut reader, mut writer) = connect(addr).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let cmd = serde_json::to_string(&ControlCommand::SetConfig {
        config: DaemonConfig {
            callsign: "N0CALL".into(),
            grid_square: "AA00".into(),
            mode: "QPSK500".into(),
            tx_attenuation_db: -6.0,
        },
    })
    .unwrap()
        + "\n";
    writer.write_all(cmd.as_bytes()).await.unwrap();

    timeout(Duration::from_secs(2), async {
        loop {
            let mut buf = String::new();
            reader.read_line(&mut buf).await.unwrap();
            if buf.contains("\"ok\"") {
                return;
            }
        }
    })
    .await
    .expect("timed out waiting for SetConfig response");

    assert_eq!(*handle.active_mode.lock().await, "QPSK500");
    assert!(
        (*handle.tx_attenuation_db.lock().await - (-6.0)).abs() < 1e-4,
        "expected -6.0 dB"
    );
}
