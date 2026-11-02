//! Through two real daemons (`spawn_bridged_pair`): the station ID rides in each station's own keying —
//! the receiver's in front of its ACK, the sender's behind its data frame — and the receiver sets the
//! ACK deadline on every OTA decode (review 2026-11-02; on-air task 18). Without the ride-along, a
//! receiver in a continuous exchange never has 3 s of quiet, so its ID is forced at the interval,
//! most likely inside the sender's frame.

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use openpulse_channel::awgn::AwgnChannel;
use openpulse_channel::AwgnConfig;
use openpulse_config::OpenpulseConfig;
use openpulse_daemon::protocol::ControlCommand;
use openpulse_daemon::twin::spawn_bridged_pair;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<u8>>>);

impl Write for Log {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn cfg(callsign: &str, tcp_port: u16, ws_port: u16) -> OpenpulseConfig {
    let mut c = OpenpulseConfig::default();
    c.station.callsign = callsign.into();
    c.modem.mode = "BPSK250".into();
    c.modem.ota_enabled = true;
    // SL6's frames are about 1 s, so several exchanges fit in the test (SL2's are 66 s).
    c.modem.ota_profile = "robust".into();
    c.modem.ota_lock_level = "SL6".into();
    c.daemon.tcp_port = tcp_port;
    c.daemon.websocket_port = ws_port;
    // A 20 s interval: the ride-along window (half of it) opens 10 s after each ID, and the exchange
    // below never leaves 3 s of quiet, so the receive tick's own early ID cannot take the slot first.
    c.station.auto_id_interval_secs = 20;
    c
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn the_station_id_rides_in_each_stations_own_keying() {
    let log = Log::default();
    let sink = log.clone();
    tracing_subscriber::fmt()
        .with_env_filter("warn,openpulse_daemon=info,openpulse_modem::ota_deadline=debug")
        .with_writer(move || sink.clone())
        .with_ansi(false)
        .init();

    let pair = spawn_bridged_pair(
        cfg("RIDEA", 19176, 19177),
        cfg("RIDEB", 19178, 19179),
        Box::new(AwgnChannel::new(AwgnConfig::new(40.0, Some(5))).unwrap()),
        Box::new(AwgnChannel::new(AwgnConfig::new(40.0, Some(6))).unwrap()),
        Duration::from_millis(10),
    )
    .await;
    let a = TcpStream::connect(pair.addr_a).await.unwrap();
    let (_a_read, mut a_write) = a.into_split();
    tokio::time::sleep(Duration::from_millis(250)).await;

    let seen = |needle: &str| String::from_utf8_lossy(&log.0.lock().unwrap()).contains(needle);
    let mut ok = false;
    for i in 0..40 {
        let cmd = serde_json::to_string(&ControlCommand::SendMessage {
            to: "RIDEB".into(),
            subject: "s".into(),
            body: format!("ride-along message {i}"),
        })
        .unwrap()
            + "\n";
        a_write.write_all(cmd.as_bytes()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(1_000)).await;
        if seen("kind=\"with-ack\"") && seen("kind=\"with-data\"") {
            ok = true;
            break;
        }
    }
    let text = String::from_utf8_lossy(&log.0.lock().unwrap()).to_string();
    // Not `pair.shutdown()`: it joins daemon A, which may still be working through queued messages.
    drop(pair);
    assert!(
        text.contains("OTA decode: ACK deadline set"),
        "the receiver's OTA decode ran without an ACK deadline (server.rs, set_ota_ack_deadline)"
    );
    assert!(
        ok,
        "the station ID did not ride in both stations' own keying: with-ack {}, with-data {}",
        text.contains("kind=\"with-ack\""),
        text.contains("kind=\"with-data\"")
    );
}
