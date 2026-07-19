//! A slow command-port client must not silently lose the TNC event stream (audit 2026-07-19, #12).
//!
//! The command loop matched `Ok(event) = event_rx.recv()`. In a `select!`, a non-matching result
//! disables that branch, so a `RecvError::Lagged` — which a broadcast channel returns when a slow
//! client falls behind the ring — took the event branch out of contention instead of being handled.
//! Its two sibling loops (`data.rs` and the bridge worker) had already been fixed for exactly this;
//! the command port was never swept.
//!
//! What travels this channel makes it matter: `DISCONNECTED` and the §97 `FAULT no MYID` line.

use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;

use openpulse_ardop::{ArdopConfig, ArdopServer, ModemBridge};
use openpulse_audio::loopback::LoopbackBackend;
use openpulse_modem::ModemEngine;

/// Start a TNC and hand back the bridge so a test can push events directly.
async fn start() -> (u16, std::sync::Arc<ModemBridge>) {
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
        loopback: true,
        ..Default::default()
    };
    let server = ArdopServer::new(engine, config);
    let bridge = server.bridge();
    tokio::spawn(async move {
        let _ = server.run_with_listeners(cmd_listener, data_listener).await;
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    (cmd_port, bridge)
}

/// THE GATE: overflow the broadcast ring, then push one more event. The client must still receive it.
///
/// Before the fix the `Lagged` error disabled the event branch, so the event after an overflow never
/// reached the client — the connection stayed up and simply went quiet.
#[tokio::test]
async fn a_lagged_client_still_receives_later_events() {
    let (port, bridge) = start().await;

    let stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut reader = BufReader::new(stream);

    tokio::time::sleep(Duration::from_millis(50)).await; // let the client subscribe first
                                                         // Force a lag: the channel holds 32, so push well past it before reading anything.
    for i in 0..200 {
        bridge.push_event(format!("FLOOD {i}"));
    }
    tokio::time::sleep(Duration::from_millis(50)).await;

    // The event that matters — the kind a host acts on.
    bridge.push_event("DISCONNECTED".to_string());

    // Read until we see it, tolerating whatever survived the ring.
    let mut saw_disconnected = false;
    for _ in 0..64 {
        let mut line = String::new();
        match timeout(Duration::from_secs(2), reader.read_line(&mut line)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(_)) => {
                if line.trim() == "DISCONNECTED" {
                    saw_disconnected = true;
                    break;
                }
            }
            _ => break,
        }
    }

    assert!(
        saw_disconnected,
        "the client never received DISCONNECTED after a broadcast lag — the event branch was \
         disabled, so the connection stays up and silently goes quiet"
    );
}

/// Control: without any lag the event stream works, so the gate above is not passing for an
/// unrelated reason.
#[tokio::test]
async fn events_reach_a_client_that_keeps_up() {
    let (port, bridge) = start().await;

    let stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut reader = BufReader::new(stream);
    // The client subscribes inside `handle_client`, which runs on a spawned task. Pushing before it
    // has subscribed sends to nobody — a race in the test, not in the code under test.
    tokio::time::sleep(Duration::from_millis(50)).await;

    bridge.push_event("DISCONNECTED".to_string());

    let mut line = String::new();
    timeout(Duration::from_secs(2), reader.read_line(&mut line))
        .await
        .expect("timeout waiting for the event")
        .expect("read");
    assert_eq!(line.trim(), "DISCONNECTED");
}

/// Control: the command port still answers commands after a lag — the loop kept running rather than
/// wedging or returning.
#[tokio::test]
async fn the_command_port_still_works_after_a_lag() {
    let (port, bridge) = start().await;

    let stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut reader = BufReader::new(stream);

    tokio::time::sleep(Duration::from_millis(50)).await; // let the client subscribe first
    for i in 0..200 {
        bridge.push_event(format!("FLOOD {i}"));
    }
    tokio::time::sleep(Duration::from_millis(50)).await;

    reader
        .get_mut()
        .write_all(b"STATE\r\n")
        .await
        .expect("write");
    reader.get_mut().flush().await.expect("flush");

    // Drain until the command response appears among any surviving flood lines.
    let mut saw_state = false;
    for _ in 0..64 {
        let mut line = String::new();
        match timeout(Duration::from_secs(2), reader.read_line(&mut line)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(_)) => {
                if line.trim().starts_with("STATE") {
                    saw_state = true;
                    break;
                }
            }
            _ => break,
        }
    }
    assert!(
        saw_state,
        "the command loop stopped answering after a broadcast lag"
    );
}
