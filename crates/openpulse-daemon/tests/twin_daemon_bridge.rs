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
