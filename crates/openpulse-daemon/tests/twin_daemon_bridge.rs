//! Full-stack twin-station validation: two REAL `openpulse-server` daemons bridged
//! through a channel in one process, driven entirely via the real control protocol.
//!
//! Unlike `openpulse-linksim` (which reimplements the policy layers) and the
//! engine-level `ota_channel_adaptation` test (which bypasses the daemon), this
//! exercises the actual daemon stack end to end: a control-protocol `SendMessage`
//! on daemon A drives the real `engine.transmit`, the bridge carries the waveform
//! through a channel model into daemon B's receive tick, and B's decode surfaces
//! as a `FrameReceived` engine event on B's control stream. This is the rig for
//! counter-checking errors that appear on air.

use std::time::Duration;

use openpulse_channel::awgn::AwgnChannel;
use openpulse_channel::AwgnConfig;
use openpulse_config::OpenpulseConfig;
use openpulse_daemon::protocol::{ControlCommand, ControlEvent};
use openpulse_daemon::twin::spawn_bridged_pair;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;

fn cfg(callsign: &str, tcp_port: u16, ws_port: u16) -> OpenpulseConfig {
    let mut c = OpenpulseConfig::default();
    c.station.callsign = callsign.into();
    c.modem.mode = "BPSK250".into();
    c.daemon.tcp_port = tcp_port;
    c.daemon.websocket_port = ws_port;
    c
}

// A near-clean channel so the plain (no-FEC) SendMessage frame decodes reliably;
// the rig's value is the full real-stack path, not a marginal-SNR stress here.
fn clean_awgn(seed: u64) -> Box<AwgnChannel> {
    Box::new(AwgnChannel::new(AwgnConfig::new(40.0, Some(seed))).unwrap())
}

/// Parse a `"SLn"` level name to its number (e.g. `"SL4"` → 4); 0 if unparseable.
fn level_num(name: &str) -> u8 {
    name.trim_start_matches("SL").parse().unwrap_or(0)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn message_crosses_the_bridge_between_two_real_daemons() {
    let pair = spawn_bridged_pair(
        cfg("DAEMONA", 19010, 19011),
        cfg("DAEMONB", 19012, 19013),
        clean_awgn(1),
        clean_awgn(2),
        Duration::from_millis(10),
    )
    .await;

    // Watch daemon B's control-event stream.
    let b = TcpStream::connect(pair.addr_b).await.unwrap();
    let (b_read, _b_write) = b.into_split();
    let mut b_reader = BufReader::new(b_read);

    // Drive a transmission from daemon A over the (bridged) air via the real
    // control protocol: SendMessage → A's run loop → engine.transmit.
    let a = TcpStream::connect(pair.addr_a).await.unwrap();
    let (_a_read, mut a_write) = a.into_split();
    // Let both control servers settle and the receive ticks start.
    tokio::time::sleep(Duration::from_millis(150)).await;
    let cmd = serde_json::to_string(&ControlCommand::SendMessage {
        to: "DAEMONB".into(),
        subject: "x".into(),
        body: "twin-station bridge hello".into(),
    })
    .unwrap()
        + "\n";
    a_write.write_all(cmd.as_bytes()).await.unwrap();

    // Daemon B should decode the frame and broadcast a FrameReceived engine event.
    let got = timeout(Duration::from_secs(15), async {
        loop {
            let mut buf = String::new();
            if b_reader.read_line(&mut buf).await.unwrap() == 0 {
                continue;
            }
            let line = buf.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(ControlEvent::EngineEvent {
                event: openpulse_modem::EngineEvent::FrameReceived { bytes, .. },
            }) = serde_json::from_str::<ControlEvent>(line)
            {
                if bytes > 0 {
                    return true;
                }
            }
        }
    })
    .await;

    pair.shutdown();
    assert!(
        got.is_ok(),
        "daemon B never decoded the frame daemon A transmitted across the bridge"
    );
}

fn ota_cfg(callsign: &str, tcp_port: u16, ws_port: u16) -> OpenpulseConfig {
    let mut c = cfg(callsign, tcp_port, ws_port);
    c.modem.ota_enabled = true;
    c.modem.ota_profile = "hpx500".into();
    c
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn ota_ladder_steps_under_traffic_between_two_real_daemons() {
    // Both daemons run a receiver-led OTA session. Driving SendMessage on A makes A
    // the ISS (transmit at the OTA mode → wait for B's ACK → adopt its absolute
    // recommended_level); B's receive tick is the IRS (decode → ACK with a
    // recommendation). Over several frames A's TX level must climb above the SL2
    // floor — i.e. the rate ladder moves, which is what the panel renders.
    let pair = spawn_bridged_pair(
        ota_cfg("OTAA", 19020, 19021),
        ota_cfg("OTAB", 19022, 19023),
        clean_awgn(11),
        clean_awgn(12),
        Duration::from_millis(10),
    )
    .await;

    let a = TcpStream::connect(pair.addr_a).await.unwrap();
    let (a_read, mut a_write) = a.into_split();
    let mut a_reader = BufReader::new(a_read);
    tokio::time::sleep(Duration::from_millis(200)).await;

    let send = serde_json::to_string(&ControlCommand::SendMessage {
        to: "OTAB".into(),
        subject: "t".into(),
        body: "ota ladder traffic frame".into(),
    })
    .unwrap()
        + "\n";

    // Drive ~10 OTA sends and track the highest TX level OtaStatus reports.
    let max_level = timeout(Duration::from_secs(40), async {
        let mut max_seen = 2u8; // SL2 floor
        for _ in 0..10 {
            a_write.write_all(send.as_bytes()).await.unwrap();
            // Read until the post-send OtaStatus (or any) reports a tx_level.
            let round = timeout(Duration::from_secs(6), async {
                loop {
                    let mut buf = String::new();
                    if a_reader.read_line(&mut buf).await.unwrap() == 0 {
                        continue;
                    }
                    let line = buf.trim();
                    if let Ok(ControlEvent::OtaStatus {
                        tx_level: Some(lvl),
                        ..
                    }) = serde_json::from_str::<ControlEvent>(line)
                    {
                        return level_num(&lvl);
                    }
                }
            })
            .await;
            if let Ok(n) = round {
                max_seen = max_seen.max(n);
            }
        }
        max_seen
    })
    .await;

    pair.shutdown();
    let max_level = max_level.expect("timed out driving OTA traffic");
    assert!(
        max_level > 2,
        "OTA rate ladder should step above the SL2 floor under traffic; reached SL{max_level}"
    );
}

/// Config with direct file transfer enabled (receiver auto-accepts any size; no handshake required).
fn ft_cfg(
    callsign: &str,
    tcp_port: u16,
    ws_port: u16,
    download_dir: &std::path::Path,
) -> OpenpulseConfig {
    let mut c = cfg(callsign, tcp_port, ws_port);
    c.file_transfer.enabled = true;
    c.file_transfer.require_verified_peer = false;
    c.file_transfer.auto_accept_max_bytes = 10_000_000;
    c.file_transfer.max_file_bytes = 10_000_000;
    c.file_transfer.download_dir = download_dir.to_string_lossy().into_owned();
    c
}

/// FF-16 Phase C acceptance: a file sent from daemon A lands, reassembled byte-for-byte, on daemon B —
/// across the real modem + a clean channel, driven entirely through the control protocol.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_file_crosses_the_bridge_between_two_real_daemons() {
    let base = std::env::temp_dir().join(format!("opfx_twin_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let recv_dir = base.join("recv");
    std::fs::create_dir_all(&base).unwrap();

    // The file A will offer (small — one block over BPSK250 keeps the test quick).
    let src = base.join("payload.txt");
    let contents = b"twin file-transfer payload across two real daemons ".repeat(4);
    std::fs::write(&src, &contents).unwrap();

    let pair = spawn_bridged_pair(
        ft_cfg("STNA", 19030, 19031, &base.join("dl_a")),
        ft_cfg("STNB", 19032, 19033, &recv_dir),
        clean_awgn(1),
        clean_awgn(2),
        Duration::from_millis(10),
    )
    .await;

    // Watch daemon B for the received file.
    let b = TcpStream::connect(pair.addr_b).await.unwrap();
    let (b_read, _b_write) = b.into_split();
    let mut b_reader = BufReader::new(b_read);

    // Drive SendFile on daemon A.
    let a = TcpStream::connect(pair.addr_a).await.unwrap();
    let (_a_read, mut a_write) = a.into_split();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let cmd = serde_json::to_string(&ControlCommand::SendFile {
        to: "STNB".into(),
        path: src.to_string_lossy().into_owned(),
    })
    .unwrap()
        + "\n";
    a_write.write_all(cmd.as_bytes()).await.unwrap();

    // Daemon B must emit FileReceived; capture the path it wrote to.
    let received = timeout(Duration::from_secs(90), async {
        loop {
            let mut buf = String::new();
            if b_reader.read_line(&mut buf).await.unwrap() == 0 {
                continue;
            }
            if let Ok(ControlEvent::FileReceived { path, name, .. }) =
                serde_json::from_str::<ControlEvent>(buf.trim())
            {
                return (path, name);
            }
        }
    })
    .await;

    pair.shutdown();

    let (path, name) = received.expect("daemon B never reported the file crossing the bridge");
    assert!(name.contains("payload"), "unexpected file name {name}");
    let got = std::fs::read(&path).expect("received file must exist on disk");
    assert_eq!(got, contents, "reassembled file must match the sent bytes");
    let _ = std::fs::remove_dir_all(&base);
}
