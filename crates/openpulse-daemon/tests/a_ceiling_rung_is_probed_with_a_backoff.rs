//! A ceiling rung that fails every frame is probed with a doubling backoff, and the anchor stays on
//! the rung that decodes (`docs/dev/design/ota-climb-memory.md`, change 1).
//!
//! On air (task 31, f2k) the receiver re-proposed SL10 after every three clean SL9 decodes, three
//! SL10 failures stepped its anchor to SL8, below the SL9 that kept decoding, and each probe spent
//! the whole NACK budget: 30 of 106 frames at SL10, 33 at SL8 under a proven SL9. Here the ceiling is
//! SL9: the forward channel time-reverses every chunk while B's last decision recommends SL9 or
//! higher, so each SL9 frame carries a frame's energy and cannot decode. Before the climb memory this
//! measured 24 SL9 probes in 40 of B's decisions and 8 decodes at SL7 under a proven SL8.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use openpulse_channel::awgn::AwgnChannel;
use openpulse_channel::{AwgnConfig, ChannelModel};
use openpulse_config::OpenpulseConfig;
use openpulse_core::ota_rate::RateDecision;
use openpulse_daemon::protocol::{ControlCommand, ControlEvent};
use openpulse_daemon::twin::spawn_bridged_pair;
use openpulse_modem::event::EngineEvent;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;

/// Clean while `reversed` is clear; every chunk the bridge moves is played backwards while it is set.
struct CeilingChannel {
    awgn: AwgnChannel,
    reversed: Arc<AtomicBool>,
}

impl ChannelModel for CeilingChannel {
    fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        let mut out = self.awgn.apply(input);
        if self.reversed.load(Ordering::Relaxed) {
            out.reverse();
        }
        out
    }
    fn generate_noise(&mut self, length: usize) -> Vec<f32> {
        self.awgn.generate_noise(length)
    }
}

fn awgn(seed: u64) -> AwgnChannel {
    AwgnChannel::new(AwgnConfig::new(40.0, Some(seed))).unwrap()
}

// This binary's own ports, 19250–19253; the sibling daemon test binaries hold 19010–19243.
fn ota_cfg(callsign: &str, tcp_port: u16) -> OpenpulseConfig {
    let mut c = OpenpulseConfig::default();
    c.station.callsign = callsign.into();
    // OFDM as the station's own mode, as `muted_failures_move_no_ladder`: a failed burst's uncoded
    // fallback scan runs at OFDM's cost.
    c.modem.mode = "OFDM52".into();
    c.daemon.tcp_port = tcp_port;
    c.daemon.websocket_port = tcp_port + 1;
    c.modem.ota_enabled = true;
    c.modem.ota_profile = "fast".into();
    c.modem.ota_min_level = "SL7".into();
    c
}

fn level_num(name: &str) -> u8 {
    name.trim_start_matches("SL").parse().unwrap_or(0)
}

/// One of B's decisions: the level it recommended before the frame, after it, what decoded.
#[derive(Debug, Clone)]
struct Decision {
    from: u8,
    to: u8,
    decoded: Option<u8>,
    decision: RateDecision,
}

const DECISIONS: usize = 40;

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_ceiling_rung_is_probed_with_a_backoff() {
    let reversed = Arc::new(AtomicBool::new(false));
    let fwd = CeilingChannel {
        awgn: awgn(41),
        reversed: reversed.clone(),
    };
    let pair = spawn_bridged_pair(
        ota_cfg("CEIA", 19250),
        ota_cfg("CEIB", 19252),
        Box::new(fwd),
        Box::new(awgn(42)),
        Duration::from_millis(10),
    )
    .await;

    let b = TcpStream::connect(pair.addr_b).await.unwrap();
    // The write halves stay alive: dropping one reads as a closed control client.
    let (b_read, _b_write) = b.into_split();
    let mut b_reader = BufReader::new(b_read);
    let a = TcpStream::connect(pair.addr_a).await.unwrap();
    let (a_read, mut a_write) = a.into_split();
    tokio::spawn(async move {
        let mut lines = BufReader::new(a_read).lines();
        while let Ok(Some(_)) = lines.next_line().await {}
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let send = serde_json::to_string(&ControlCommand::SendMessage {
        to: "CEIB".into(),
        subject: "t".into(),
        body: "a ceiling rung is probed with a backoff".into(),
    })
    .unwrap()
        + "\n";

    // A's sends run one after another in its run loop, so a few queued keep it sending; one more is
    // queued per decode (a delivered message), and on a timer in case a send is abandoned.
    let log = timeout(Duration::from_secs(900), async {
        let mut log: Vec<Decision> = Vec::new();
        for _ in 0..3 {
            a_write.write_all(send.as_bytes()).await.unwrap();
        }
        let mut refill = tokio::time::Instant::now() + Duration::from_secs(30);
        while log.len() < DECISIONS {
            let mut buf = String::new();
            let read = tokio::time::timeout_at(refill, b_reader.read_line(&mut buf)).await;
            let Ok(Ok(n)) = read else {
                a_write.write_all(send.as_bytes()).await.unwrap();
                refill = tokio::time::Instant::now() + Duration::from_secs(30);
                continue;
            };
            assert!(n > 0, "B's control stream closed");
            let Ok(ControlEvent::EngineEvent {
                event:
                    EngineEvent::OtaRateDecision {
                        from,
                        to,
                        decoded_level,
                        decision,
                        ..
                    },
            }) = serde_json::from_str::<ControlEvent>(buf.trim())
            else {
                continue;
            };
            let d = Decision {
                from: level_num(&from.name()),
                to: level_num(&to.name()),
                decoded: decoded_level.map(|l| level_num(&l.name())),
                decision,
            };
            // The ceiling: what B now expects at SL9 or above cannot decode.
            reversed.store(d.to >= 9, Ordering::Relaxed);
            if d.decoded.is_some() {
                a_write.write_all(send.as_bytes()).await.unwrap();
                refill = tokio::time::Instant::now() + Duration::from_secs(30);
            }
            log.push(d);
        }
        log
    })
    .await;
    pair.shutdown();
    let log = log.expect("B never logged 40 decisions");

    // A probe of SL9 is a frame B expected at SL9 (`from` is B's recommendation before it).
    let probes = log.iter().filter(|d| d.from >= 9).count();
    let first_sl8 = log
        .iter()
        .position(|d| d.decoded == Some(8))
        .expect("B never decoded SL8: the climb to the proven rung did not happen");
    let below = log[first_sl8..]
        .iter()
        .filter(|d| d.decoded.is_some_and(|l| l < 8))
        .count();
    let withdrawn = log
        .iter()
        .filter(|d| d.decision == RateDecision::ProbeWithdrawn)
        .count();
    eprintln!("SL9 probes {probes}, decodes below SL8 {below}, withdrawn {withdrawn}");
    assert!(
        probes <= 4,
        "{probes} SL9 probes in {DECISIONS} decisions (24 before the climb memory): {log:?}"
    );
    assert_eq!(
        below, 0,
        "{below} decodes below the proven SL8 after it decoded (8 before the climb memory): {log:?}"
    );
    assert!(
        withdrawn >= 1
            && log
                .iter()
                .any(|d| d.decision == RateDecision::ClimbDeclined),
        "the ceiling must have been probed, withdrawn and declined: {log:?}"
    );
}
