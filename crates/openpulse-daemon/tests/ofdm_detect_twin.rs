//! Gates 9 and 11 of `docs/dev/design/ofdm-preamble-detect.md`, through two real daemons
//! (`server::run`): an SL7 frame at the level a receiver AGC leaves it — total power 1.15× the hiss
//! the receiver hears between frames — is gathered by the preamble detect P, decoded, and answered
//! with an ACK the sender hears inside its window.
//!
//! - Gate 9, the tripwire on the production path: the receiving daemon's `preamble carrier detect`
//!   line reports P's blocks and opens moving.
//! - Gate 11, the ACK end to end: the sender logs `OTA ACK received` for the message. Gates 1–10 do
//!   not cross the ACK, and the ACK is the point of the feature.
//!
//! The twin delivers a frame faster than real time, so the receiver's wall-clock ACK deadline is off
//! here (`server.rs`, `tick_real_time`); what bounds the ACK is the sender's own listen window.

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use openpulse_channel::awgn::AwgnChannel;
use openpulse_channel::{AwgnConfig, ChannelModel};
use openpulse_config::OpenpulseConfig;
use openpulse_daemon::protocol::{ControlCommand, ControlEvent};
use openpulse_daemon::twin::spawn_bridged_pair_in_hiss;
use openpulse_modem::event::EngineEvent;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;

#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<u8>>>);

impl Write for Log {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log").extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Log {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().expect("log")).to_string()
    }
}

/// The hiss level between frames, RMS.
const HISS: f32 = 0.05;

/// A receiver AGC, as the IC-9700's AGC FAST behaved in run f2g: between frames the hiss at `HISS`;
/// under a frame the hiss 12 dB down and the frame scaled so the total is 1.15× the gap hiss.
struct AgcChannel {
    state: u64,
}

impl AgcChannel {
    fn gauss(&mut self) -> f32 {
        let mut u = || {
            self.state = self
                .state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((self.state >> 40) as f32 + 1.0) / 16_777_217.0
        };
        let (a, b) = (u(), u());
        (-2.0 * a.ln()).sqrt() * (std::f32::consts::TAU * b).cos()
    }
}

impl ChannelModel for AgcChannel {
    fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        if input.is_empty() {
            return Vec::new();
        }
        let rms = (input.iter().map(|v| v * v).sum::<f32>() / input.len() as f32).sqrt();
        let under = 0.25f32;
        let g = if rms > 0.0 {
            ((1.15f32 * 1.15 - under * under).sqrt() * HISS) / rms
        } else {
            0.0
        };
        input
            .iter()
            .map(|v| v * g + under * HISS * self.gauss())
            .collect()
    }
    fn generate_noise(&mut self, length: usize) -> Vec<f32> {
        (0..length).map(|_| HISS * self.gauss()).collect()
    }
}

// This binary's own ports, 19240–19243.
fn cfg(callsign: &str, tcp_port: u16) -> OpenpulseConfig {
    let mut c = OpenpulseConfig::default();
    c.station.callsign = callsign.into();
    c.modem.mode = "OFDM52".into();
    c.daemon.tcp_port = tcp_port;
    c.daemon.websocket_port = tcp_port + 1;
    c.modem.ota_enabled = true;
    c.modem.ota_profile = "fast".into();
    c.modem.ota_min_level = "SL7".into();
    c.modem.ota_max_level = "SL7".into();
    c.station.auto_id_interval_secs = 0;
    c
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn an_sl7_frame_at_agc_level_is_gathered_decoded_and_acked() {
    let log = Log::default();
    let sink = log.clone();
    tracing_subscriber::fmt()
        .with_env_filter("openpulse_daemon=info,openpulse_modem=info")
        .with_writer(move || sink.clone())
        .with_ansi(false)
        .init();

    let pair = spawn_bridged_pair_in_hiss(
        cfg("PDTA", 19240),
        cfg("PDTB", 19242),
        Box::new(AgcChannel { state: 41 }),
        Box::new(AwgnChannel::new(AwgnConfig::new(40.0, Some(42))).expect("awgn")),
        Duration::from_millis(10),
    )
    .await;
    let a = TcpStream::connect(pair.addr_a).await.expect("A");
    let (_a_read, mut a_write) = a.into_split();
    let b = TcpStream::connect(pair.addr_b).await.expect("B");
    let (b_read, _b_write) = b.into_split();
    let mut b_reader = BufReader::new(b_read);
    // B hears the hiss for 2 s before anything is sent, so its floor rests on the band.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let send = serde_json::to_string(&ControlCommand::SendMessage {
        to: "PDTB".into(),
        subject: "t".into(),
        body: "an SL7 frame under the AGC".into(),
    })
    .expect("json")
        + "\n";
    a_write.write_all(send.as_bytes()).await.expect("send");

    let outcome = timeout(Duration::from_secs(90), async {
        let mut decoded_at_sl7 = false;
        loop {
            let mut buf = String::new();
            let Ok(Ok(n)) =
                tokio::time::timeout(Duration::from_millis(500), b_reader.read_line(&mut buf))
                    .await
            else {
                if decoded_at_sl7 && log.text().contains("OTA ACK received") {
                    return true;
                }
                continue;
            };
            if n == 0 {
                return false;
            }
            if let Ok(ControlEvent::EngineEvent {
                event:
                    EngineEvent::OtaRateDecision {
                        decoded_level: Some(l),
                        ..
                    },
            }) = serde_json::from_str::<ControlEvent>(buf.trim())
            {
                decoded_at_sl7 |= l.name() == "SL7";
            }
            if decoded_at_sl7 && log.text().contains("OTA ACK received") {
                return true;
            }
        }
    })
    .await;
    let stats = pair.stats();
    pair.shutdown();
    let text = log.text();
    let field = |line: &str, name: &str| -> u64 {
        line.split_whitespace()
            .find_map(|w| w.strip_prefix(&format!("{name}=")))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    };
    let p_line = text
        .lines()
        .find(|l| l.contains("preamble carrier detect") && field(l, "opens") >= 1);
    assert!(
        outcome == Ok(true),
        "the SL7 frame at AGC level was not decoded and ACKed end to end ({stats}); P's line: \
         {p_line:?}"
    );
    // Gate 9: the tripwire moved on the production path.
    let Some(p_line) = p_line else {
        let relevant: Vec<&str> = text
            .lines()
            .filter(|l| l.contains("preamble") || l.contains("burst") || l.contains("ACK"))
            .collect();
        panic!("the receiving daemon logged no P line with an open: {relevant:#?}");
    };
    assert!(field(p_line, "blocks") > 0, "P judged no block: {p_line}");
    assert!(
        !text.contains("decode finished past the sender's ACK window"),
        "the receiver's decode ran past the ACK window"
    );
}
