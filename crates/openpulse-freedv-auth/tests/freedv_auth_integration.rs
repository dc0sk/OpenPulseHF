//! Integration tests for openpulse-freedv-auth.

use ed25519_dalek::SigningKey;
use openpulse_freedv_auth::{
    beacon::AuthBeacon,
    data_port::FreeDvDataPort,
    scheduler::BeaconScheduler,
    verdict::{SharedVerdict, TrustVerdict, VerdictServer},
};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::net::UdpSocket;

fn make_key_pair() -> ([u8; 32], [u8; 32]) {
    let seed = [0xBEu8; 32];
    let sk = SigningKey::from_bytes(&seed);
    (seed, sk.verifying_key().to_bytes())
}

/// Beacon sign → encode → decode → verify round-trip.
#[test]
fn beacon_full_round_trip() {
    let (seed, pubkey) = make_key_pair();
    let nonce = [0xCAu8; 16];
    let beacon = AuthBeacon::sign(
        "N0CALL",
        1_746_800_000,
        nonce,
        14_236_000,
        "FreeDV-1600",
        &seed,
        pubkey,
    );
    assert!(beacon.verify(), "signature must verify");

    let wire = beacon.encode();
    let decoded = AuthBeacon::decode(&wire).unwrap();
    assert_eq!(beacon, decoded);
    assert!(decoded.verify(), "decoded signature must verify");
}

/// Tampered beacon fails verification.
#[test]
fn tampered_beacon_fails() {
    let (seed, pubkey) = make_key_pair();
    let mut beacon = AuthBeacon::sign(
        "W1AW",
        1_746_800_000,
        [0u8; 16],
        14_236_000,
        "FreeDV-1600",
        &seed,
        pubkey,
    );
    beacon.freq_hz = 7_074_000; // tamper after signing
    assert!(!beacon.verify());
}

/// BeaconScheduler fires once and the mock server receives a valid beacon.
#[tokio::test]
async fn scheduler_sends_beacon_to_udp_server() {
    let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let server_addr = server.local_addr().unwrap().to_string();

    let port = Arc::new(FreeDvDataPort::connect(&server_addr).await.unwrap());
    let (seed, pubkey) = make_key_pair();

    let scheduler = BeaconScheduler::new(
        Duration::from_secs(3600), // effectively one-shot for this test
        Arc::clone(&port),
        move || {
            AuthBeacon::sign(
                "KD9ZZZ",
                1_746_800_000,
                [0x77u8; 16],
                14_074_000,
                "FreeDV-1600",
                &seed,
                pubkey,
            )
        },
    );

    // Run scheduler in background; it fires immediately.
    tokio::spawn(async move { scheduler.run().await });

    let mut buf = [0u8; 1024];
    let (n, _) = tokio::time::timeout(Duration::from_millis(500), server.recv_from(&mut buf))
        .await
        .expect("timed out waiting for beacon")
        .unwrap();

    let decoded = AuthBeacon::decode(&buf[..n]).unwrap();
    assert_eq!(decoded.callsign, "KD9ZZZ");
    assert!(decoded.verify());
}

/// VerdictServer writes the current verdict as JSON to connecting clients.
#[tokio::test]
async fn verdict_server_serves_json() {
    use tokio::io::AsyncReadExt;
    use tokio::net::UnixStream;

    let path = std::env::temp_dir().join(format!(
        "openpulse_verdict_test_{}.sock",
        std::process::id()
    ));

    let verdict: SharedVerdict = Arc::new(RwLock::new(TrustVerdict::Verified {
        callsign: "W1AW".into(),
        key_id: "aabbccdd".into(),
        last_beacon_utc: 1_746_800_000,
    }));

    let server = VerdictServer::new(path.clone(), Arc::clone(&verdict));
    tokio::spawn(server.serve());
    tokio::time::sleep(Duration::from_millis(20)).await;

    let mut stream = UnixStream::connect(&path).await.unwrap();
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_millis(300), stream.read_to_end(&mut buf)).await;

    let text = String::from_utf8(buf).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(parsed["verdict"], "verified");
    assert_eq!(parsed["callsign"], "W1AW");

    let _ = std::fs::remove_file(&path);
}

/// Unverified default verdict serialises correctly.
#[test]
fn unverified_default_verdict() {
    let v = TrustVerdict::default();
    let json = serde_json::to_string(&v).unwrap();
    assert!(json.contains("unverified"));
}
