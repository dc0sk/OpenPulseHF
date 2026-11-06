//! A climb answered by silence: the sender returns to the level before it and the message is
//! delivered there (`docs/dev/design/ladder-climb-into-silence.md`).
//!
//! On air (task 23, run f2c) the climb SL7 → SL8 dropped the sender's audio by 13 dB. The receiver
//! heard nothing, keyed nothing, and the link never recovered: B abandoned 51 messages at SL8. Here
//! the forward channel carries only noise from the moment A's transmit level reaches SL8 until A
//! leaves it, so the receiver hears nothing at SL8, as on air.
//!
//! On air (tasks 24–26) the fall-back never fired: the runner polled `ota-status` every ~5 s, and the
//! CLI's status query was a bounds command with the bounds already in force, which emptied the
//! sender's climb record. The polled test below sends that command throughout.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use openpulse_channel::awgn::AwgnChannel;
use openpulse_channel::{AwgnConfig, ChannelModel};
use openpulse_config::OpenpulseConfig;
use openpulse_daemon::protocol::{ControlCommand, ControlEvent};
use openpulse_daemon::twin::spawn_bridged_pair;
use openpulse_modem::event::EngineEvent;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;

/// Clean, or what the channel makes of silence while `silent` is set: the frame never reaches the
/// receiver, and nothing else does either.
struct MutingChannel {
    awgn: AwgnChannel,
    silent: Arc<AtomicBool>,
}

impl ChannelModel for MutingChannel {
    fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        if self.silent.load(Ordering::Relaxed) {
            // Exactly what the channel makes of silence: louder noise would be a burst the receiver
            // fails and NACKs (a first version did that, and B stepped down on its own).
            self.awgn.apply(&vec![0.0; input.len()])
        } else {
            self.awgn.apply(input)
        }
    }
    fn generate_noise(&mut self, length: usize) -> Vec<f32> {
        self.awgn.generate_noise(length)
    }
}

fn awgn(seed: u64) -> AwgnChannel {
    AwgnChannel::new(AwgnConfig::new(40.0, Some(seed))).unwrap()
}

// This binary's own ports, 19230–19237; the sibling daemon test binaries hold 19010–19223.
fn ota_cfg(callsign: &str, tcp_port: u16) -> OpenpulseConfig {
    let mut c = OpenpulseConfig::default();
    c.station.callsign = callsign.into();
    // OFDM as the station's own mode, as `muted_failures_move_no_ladder`: a failed burst's uncoded
    // fallback scan runs at OFDM's cost, which keeps a debug build's run short.
    c.modem.mode = "OFDM52".into();
    c.daemon.tcp_port = tcp_port;
    c.daemon.websocket_port = tcp_port + 1;
    c.modem.ota_enabled = true;
    c.modem.ota_profile = "fast".into();
    c.modem.ota_min_level = "SL7".into();
    c
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_climb_answered_by_silence_falls_back_and_delivers() {
    assert_eq!(
        climb_into_silence(19230, false).await,
        Ok(true),
        "A never fell back from the silent SL8 to SL7 and delivered there"
    );
}

/// As the runner's status poll before the fix: the bounds A already runs with, every 3 s.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_status_poll_with_unchanged_bounds_keeps_the_fall_back() {
    assert_eq!(
        climb_into_silence(19234, true).await,
        Ok(true),
        "with A polled, A never fell back from the silent SL8 to SL7 and delivered there"
    );
}

async fn climb_into_silence(port: u16, poll: bool) -> Result<bool, tokio::time::error::Elapsed> {
    let silent = Arc::new(AtomicBool::new(false));
    let fwd = MutingChannel {
        awgn: awgn(31),
        silent: silent.clone(),
    };
    let pair = spawn_bridged_pair(
        ota_cfg("SILA", port),
        ota_cfg("SILB", port + 2),
        Box::new(fwd),
        Box::new(awgn(32)),
        Duration::from_millis(10),
    )
    .await;

    let a = TcpStream::connect(pair.addr_a).await.unwrap();
    let (a_read, mut a_write) = a.into_split();
    let mut a_reader = BufReader::new(a_read);
    let b = TcpStream::connect(pair.addr_b).await.unwrap();
    let (b_read, _b_write) = b.into_split();
    let mut b_reader = BufReader::new(b_read);
    if poll {
        let p = TcpStream::connect(pair.addr_a).await.unwrap();
        let (p_read, mut p_write) = p.into_split();
        let bounds = serde_json::to_string(&ControlCommand::OtaSetLevelBounds {
            min_level: Some("SL7".into()),
            max_level: None,
        })
        .unwrap()
            + "\n";
        tokio::spawn(async move {
            let mut lines = BufReader::new(p_read).lines();
            while let Ok(Some(_)) = lines.next_line().await {}
        });
        tokio::spawn(async move {
            while p_write.write_all(bounds.as_bytes()).await.is_ok() {
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        });
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let send = serde_json::to_string(&ControlCommand::SendMessage {
        to: "SILB".into(),
        subject: "t".into(),
        body: "a climb into silence".into(),
    })
    .unwrap()
        + "\n";

    // B's decodes, in order: the level each frame decoded at.
    let (b_tx, mut b_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        let mut buf = String::new();
        while let Ok(n) = b_reader.read_line(&mut buf).await {
            if n == 0 {
                break;
            }
            if let Ok(ControlEvent::EngineEvent {
                event:
                    EngineEvent::OtaRateDecision {
                        decoded_level: Some(l),
                        ..
                    },
            }) = serde_json::from_str::<ControlEvent>(buf.trim())
            {
                let _ = b_tx.send(l.name());
            }
            buf.clear();
        }
    });

    let outcome = timeout(Duration::from_secs(400), async {
        let mut muted = false;
        let mut fell_back = false;
        loop {
            a_write.write_all(send.as_bytes()).await.unwrap();
            let round_end = tokio::time::Instant::now() + Duration::from_secs(40);
            loop {
                let mut buf = String::new();
                let read = tokio::time::timeout_at(round_end, a_reader.read_line(&mut buf)).await;
                let Ok(Ok(n)) = read else { break };
                if n == 0 {
                    break;
                }
                if let Ok(ControlEvent::OtaStatus {
                    tx_level: Some(level),
                    ..
                }) = serde_json::from_str::<ControlEvent>(buf.trim())
                {
                    if !muted && !fell_back && level == "SL8" {
                        // The climb: from here nothing A sends reaches B, until A leaves SL8.
                        muted = true;
                        silent.store(true, Ordering::Relaxed);
                    } else if muted && level == "SL7" {
                        muted = false;
                        fell_back = true;
                        silent.store(false, Ordering::Relaxed);
                    }
                }
            }
            if fell_back {
                // A decode at SL7 after the fall-back: the message got through below the climb.
                while let Ok(l) = b_rx.try_recv() {
                    if l == "SL7" {
                        return true;
                    }
                }
            } else {
                while b_rx.try_recv().is_ok() {}
            }
        }
    })
    .await;

    pair.shutdown();
    outcome
}
