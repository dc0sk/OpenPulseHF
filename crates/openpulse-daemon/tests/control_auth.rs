//! Integration tests for the PSK-authenticated, encrypted control channel (REQ-SEC-CTL-01/02).
//!
//! A real `AsyncNoise` client connects to a real `ControlServer` configured with a PSK, over an
//! actual TCP socket, and exchanges encrypted messages; a wrong-PSK client is dropped (fail closed).

use std::net::SocketAddr;
use std::time::Duration;

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_daemon::{ControlServer, ControlServerConfig, ControlServerHandle};
use openpulse_linksec::async_channel::AsyncNoise;
use openpulse_modem::ModemEngine;
use tokio::net::TcpStream;
use tokio::time::timeout;

const PSK: [u8; 32] = [0xABu8; 32];

fn make_engine() -> ModemEngine {
    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
    engine.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    engine
}

async fn spawn_auth_server(
    engine: &ModemEngine,
    psk: Option<[u8; 32]>,
) -> (SocketAddr, ControlServerHandle) {
    let mut addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let handle = ControlServer::spawn(
        "127.0.0.1:0".parse().unwrap(),
        engine,
        ControlServerConfig {
            initial_mode: "BPSK250".into(),
            initial_station_id: ("N0CALL".into(), "AA00".into()),
            initial_qsy_enabled: false,
            initial_bandplan_mode: "unrestricted".into(),
            initial_allow_tuner_on_high_swr: false,
            control_psk: psk,
        },
        Some(&mut addr),
    )
    .await
    .unwrap();
    (addr, handle)
}

#[tokio::test]
async fn noise_client_exchanges_encrypted_messages() {
    let engine = make_engine();
    let (addr, _handle) = spawn_auth_server(&engine, Some(PSK)).await;

    let stream = TcpStream::connect(addr).await.unwrap();
    let mut ch = AsyncNoise::initiator(stream, &PSK)
        .await
        .expect("PSK handshake should succeed with the correct key");

    // Send an (intentionally invalid) command line — the server replies with a parse-error
    // CommandResponse, which exercises decrypt-command + encrypt-response end to end.
    ch.send(b"not valid json").await.unwrap();

    let msg = timeout(Duration::from_secs(3), ch.recv())
        .await
        .expect("a reply should arrive")
        .expect("recv should decrypt");
    let s = String::from_utf8(msg).expect("utf8");
    // Whatever arrives first (the parse-error response or a periodic event) must be valid NDJSON,
    // proving the encrypted channel round-trips.
    let _: serde_json::Value =
        serde_json::from_str(s.trim()).expect("decrypted message must be valid JSON");
}

#[tokio::test]
async fn wrong_psk_client_is_dropped() {
    let engine = make_engine();
    let (addr, _handle) = spawn_auth_server(&engine, Some(PSK)).await;

    let stream = TcpStream::connect(addr).await.unwrap();
    let bad = [0x11u8; 32];

    // The client either fails the handshake outright, or (if its m1 was sent before the server
    // dropped it) fails to receive anything. Either way it must never get a decryptable message.
    match timeout(Duration::from_secs(3), AsyncNoise::initiator(stream, &bad)).await {
        Ok(Ok(mut ch)) => {
            let r = timeout(Duration::from_secs(2), ch.recv()).await;
            assert!(
                !matches!(r, Ok(Ok(_))),
                "a wrong-PSK client must not receive any control message"
            );
        }
        Ok(Err(_)) => { /* handshake rejected — the expected fail-closed outcome */ }
        Err(_) => { /* handshake stalled because the server dropped us — also fail-closed */ }
    }
}
