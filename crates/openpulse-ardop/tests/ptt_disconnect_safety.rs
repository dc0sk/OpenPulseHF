//! A disconnecting host must not leave the transmitter keyed (audit 2026-07-19, finding #1).
//!
//! The ARDOP command port exposes `PTT TRUE` on an unauthenticated socket. If the host application
//! crashes, is killed, or loses the network after keying, nothing in the TNC ever released PTT — the
//! rig transmits until a human notices. That is a §97 violation and a PA-damage risk, not just a bug.
//! The daemon solved the same hazard with a watchdog (issue #863); the TNC had no equivalent.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;

use openpulse_ardop::{ArdopConfig, ArdopServer};
use openpulse_audio::loopback::LoopbackBackend;
use openpulse_modem::ModemEngine;
use openpulse_radio::{PttController, PttError};

/// A PTT controller that records its state where the test can see it.
struct SpyPtt {
    asserted: Arc<AtomicBool>,
    releases: Arc<AtomicUsize>,
}

impl PttController for SpyPtt {
    fn assert_ptt(&mut self) -> Result<(), PttError> {
        self.asserted.store(true, Ordering::SeqCst);
        Ok(())
    }
    fn release_ptt(&mut self) -> Result<(), PttError> {
        self.asserted.store(false, Ordering::SeqCst);
        self.releases.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn is_asserted(&self) -> bool {
        self.asserted.load(Ordering::SeqCst)
    }
}

/// Start a TNC whose PTT state the caller can observe. Returns (cmd_port, asserted, releases).
async fn start_spy_server() -> (u16, Arc<AtomicBool>, Arc<AtomicUsize>) {
    let cmd_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let data_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cmd_port = cmd_listener.local_addr().unwrap().port();
    let data_port = data_listener.local_addr().unwrap().port();

    let asserted = Arc::new(AtomicBool::new(false));
    let releases = Arc::new(AtomicUsize::new(0));
    let ptt = SpyPtt {
        asserted: asserted.clone(),
        releases: releases.clone(),
    };

    let engine = ModemEngine::new(Box::new(LoopbackBackend::default()));
    let config = ArdopConfig {
        bind_addr: "127.0.0.1".into(),
        command_port: cmd_port,
        data_port,
        mode: "BPSK250".into(),
        loopback: true,
        ..Default::default()
    };
    let server =
        ArdopServer::with_trust_relay_ptt(engine, config, Default::default(), None, Box::new(ptt));
    tokio::spawn(async move {
        let _ = server.run_with_listeners(cmd_listener, data_listener).await;
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    (cmd_port, asserted, releases)
}

async fn send(reader: &mut BufReader<TcpStream>, line: &str) -> String {
    reader
        .get_mut()
        .write_all(format!("{line}\r\n").as_bytes())
        .await
        .unwrap();
    reader.get_mut().flush().await.unwrap();
    let mut resp = String::new();
    timeout(Duration::from_secs(2), reader.read_line(&mut resp))
        .await
        .expect("timeout waiting for response")
        .unwrap();
    resp.trim().to_string()
}

/// Wait up to `ms` for the transmitter to unkey, polling — the release happens on the server task.
async fn wait_unkeyed(asserted: &Arc<AtomicBool>, ms: u64) -> bool {
    for _ in 0..(ms / 10) {
        if !asserted.load(Ordering::SeqCst) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    !asserted.load(Ordering::SeqCst)
}

/// THE GATE: a host that keys the transmitter and then vanishes must not leave it keyed.
#[tokio::test]
async fn dropping_the_connection_while_keyed_releases_ptt() {
    let (port, asserted, _releases) = start_spy_server().await;

    let stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut reader = BufReader::new(stream);

    assert_eq!(send(&mut reader, "PTT TRUE").await, "PTT TRUE");
    assert!(
        asserted.load(Ordering::SeqCst),
        "precondition: PTT TRUE must key the transmitter, else this test proves nothing"
    );

    // The host vanishes without sending PTT FALSE — crash, kill -9, or a dropped network.
    drop(reader);

    assert!(
        wait_unkeyed(&asserted, 1000).await,
        "transmitter still keyed after the client disconnected — a stuck carrier"
    );
}

/// Control: a clean `PTT FALSE` still works, and the disconnect path does not double-release.
#[tokio::test]
async fn explicit_ptt_false_releases_once_and_disconnect_does_not_double_release() {
    let (port, asserted, releases) = start_spy_server().await;

    let stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut reader = BufReader::new(stream);

    assert_eq!(send(&mut reader, "PTT TRUE").await, "PTT TRUE");
    assert_eq!(send(&mut reader, "PTT FALSE").await, "PTT FALSE");
    assert!(!asserted.load(Ordering::SeqCst), "PTT FALSE must unkey");
    assert_eq!(
        releases.load(Ordering::SeqCst),
        1,
        "exactly one release so far"
    );

    drop(reader);
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(
        releases.load(Ordering::SeqCst),
        1,
        "disconnect must not issue a redundant release when PTT is already down"
    );
}

/// Control: a client that never keys must not touch PTT on disconnect.
#[tokio::test]
async fn disconnect_without_keying_does_not_touch_ptt() {
    let (port, asserted, releases) = start_spy_server().await;

    let stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut reader = BufReader::new(stream);
    assert_eq!(send(&mut reader, "STATE").await, "STATE DISC");
    drop(reader);
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert!(!asserted.load(Ordering::SeqCst));
    assert_eq!(
        releases.load(Ordering::SeqCst),
        0,
        "no release should be issued for a client that never keyed"
    );
}
