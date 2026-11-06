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

/// A receiver AGC, as the IC-9700's AGC FAST behaved in run f2g: between frames the recorded gap hiss
/// of that run; under a frame that hiss 12 dB down and the frame scaled so the total is 1.15× the gap.
/// Recorded, not white: the rig's filter shapes the hiss, and white hiss spread over 0–4 kHz would put
/// the OFDM band's density 2.5× over it, where the spectral test opens on the frame and P is not
/// needed — the fixture would no longer be the case it exists for.
///
/// The frame is shaped to the hiss's own spectrum (an all-pole fit to the hiss, order 16), as the
/// rig's receive filter shapes both on air; otherwise the flat OFDM band would stand over the hiss at
/// the filter's skirts, where the spectral test opens on it.
struct AgcChannel {
    hiss: Vec<f32>,
    at: usize,
    rms: f32,
    /// The all-pole shaping filter's coefficients `a[1..]` and its state.
    lpc: Vec<f32>,
    state: Vec<f32>,
}

/// Levinson–Durbin: the order-`p` predictor of `x`, `a[1..=p]` with `x[n] ≈ −Σ a[k]·x[n−k]`.
fn lpc(x: &[f32], p: usize) -> Vec<f32> {
    let r: Vec<f64> = (0..=p)
        .map(|k| {
            x.iter()
                .zip(&x[k..])
                .map(|(a, b)| f64::from(*a) * f64::from(*b))
                .sum()
        })
        .collect();
    let mut a = vec![0.0f64; p + 1];
    a[0] = 1.0;
    let mut err = r[0];
    for i in 1..=p {
        let acc: f64 = (0..i).map(|j| a[j] * r[i - j]).sum();
        let k = -acc / err;
        let prev = a.clone();
        for j in 1..i {
            a[j] = prev[j] + k * prev[i - j];
        }
        a[i] = k;
        err *= 1.0 - k * k;
    }
    a[1..].iter().map(|&v| v as f32).collect()
}

impl AgcChannel {
    fn new() -> Self {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../openpulse-modem/tests/captures/ic9700-ofdm52-f2g-agc-1.wav"
        );
        let x = openpulse_modem::capture_replay::load_wav(path)
            .expect("the f2g excerpt loads")
            .samples;
        // The gap between the excerpt's two frames, 11.0–20.7 s: hiss only (tests/captures/README.md).
        let hiss = x[88_000..165_600].to_vec();
        let rms = (hiss.iter().map(|v| v * v).sum::<f32>() / hiss.len() as f32).sqrt();
        let lpc = lpc(&hiss, 16);
        let state = vec![0.0; lpc.len()];
        Self {
            hiss,
            at: 0,
            rms,
            lpc,
            state,
        }
    }
    /// The frame through the all-pole filter `1/A(z)`.
    fn shape(&mut self, x: &[f32]) -> Vec<f32> {
        x.iter()
            .map(|&v| {
                let y = v - self
                    .lpc
                    .iter()
                    .zip(&self.state)
                    .map(|(a, s)| a * s)
                    .sum::<f32>();
                self.state.rotate_right(1);
                self.state[0] = y;
                y
            })
            .collect()
    }
    fn next(&mut self) -> f32 {
        let v = self.hiss[self.at % self.hiss.len()];
        self.at += 1;
        v
    }
}

impl ChannelModel for AgcChannel {
    fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        if input.is_empty() {
            return Vec::new();
        }
        let input = self.shape(input);
        let rms = (input.iter().map(|v| v * v).sum::<f32>() / input.len() as f32).sqrt();
        let under = 0.25f32;
        let g = if rms > 0.0 {
            ((1.15f32 * 1.15 - under * under).sqrt() * self.rms) / rms
        } else {
            0.0
        };
        input.iter().map(|v| v * g + under * self.next()).collect()
    }
    fn generate_noise(&mut self, length: usize) -> Vec<f32> {
        (0..length).map(|_| self.next()).collect()
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
        Box::new(AgcChannel::new()),
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
