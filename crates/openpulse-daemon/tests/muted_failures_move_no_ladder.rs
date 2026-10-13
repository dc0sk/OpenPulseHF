//! #1460 round 4b through the production entry: a failed burst moves the ladder only when its NACK
//! is keyed (`docs/dev/design/failed-burst-ownership.md`, *Round 4b*).
//!
//! Before 4b every counted failure ran the NACK hysteresis, keyed or not. The budget keys three NACKs
//! without a decode, so the third announced one step and the sixth took a second step that no NACK
//! carried: the receiver stopped trying the rung the sender had been told. The verdict is computed in
//! `server.rs`'s rx tick, which an engine-level test cannot reach, so this runs two real daemons.
//!
//! B climbs on clean frames, then the forward channel time-reverses everything A sends: the same
//! energy and spectrum, so each burst still counts as a failed ladder frame, and nothing decodes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use openpulse_channel::awgn::AwgnChannel;
use openpulse_channel::{AwgnConfig, ChannelModel};
use openpulse_config::OpenpulseConfig;
use openpulse_core::ota_rate::{NackVerdict, RateDecision};
use openpulse_daemon::protocol::{ControlCommand, ControlEvent};
use openpulse_daemon::twin::spawn_bridged_pair;
use openpulse_modem::event::EngineEvent;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;

/// Clean until `reversed` is set, then every chunk the bridge moves is played backwards.
struct ReversingChannel {
    awgn: AwgnChannel,
    reversed: Arc<AtomicBool>,
}

impl ChannelModel for ReversingChannel {
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

// This binary's own ports, 19220–19223; the sibling daemon test binaries hold 19010–19211.
fn ota_cfg(callsign: &str, tcp_port: u16) -> OpenpulseConfig {
    let mut c = OpenpulseConfig::default();
    c.station.callsign = callsign.into();
    // OFDM as the station's own mode, so a failed burst's uncoded-fallback scan (#1123) runs at
    // OFDM's cost; at BPSK250 one failed burst took minutes in a debug build.
    c.modem.mode = "OFDM52".into();
    c.daemon.tcp_port = tcp_port;
    c.daemon.websocket_port = tcp_port + 1;
    c.modem.ota_enabled = true;
    c.modem.ota_profile = "fast".into();
    // SL7 floor, as `twin_daemon_bridge`'s OTA tests: OFDM frames keep the run short.
    c.modem.ota_min_level = "SL7".into();
    c
}

fn level_num(name: &str) -> u8 {
    name.trim_start_matches("SL").parse().unwrap_or(0)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn failures_after_the_budget_mutes_take_no_silent_step() {
    let reversed = Arc::new(AtomicBool::new(false));
    let fwd = ReversingChannel {
        awgn: awgn(21),
        reversed: reversed.clone(),
    };
    let pair = spawn_bridged_pair(
        ota_cfg("MUTA", 19220),
        ota_cfg("MUTB", 19222),
        Box::new(fwd),
        Box::new(awgn(22)),
        Duration::from_millis(10),
    )
    .await;

    let b = TcpStream::connect(pair.addr_b).await.unwrap();
    // The write half stays alive: dropping it reads as a closed control client.
    let (b_read, _b_write) = b.into_split();
    let mut b_reader = BufReader::new(b_read);
    let a = TcpStream::connect(pair.addr_a).await.unwrap();
    let (_a_read, mut a_write) = a.into_split();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let send = serde_json::to_string(&ControlCommand::SendMessage {
        to: "MUTB".into(),
        subject: "t".into(),
        body: "muted failures move no ladder".into(),
    })
    .unwrap()
        + "\n";

    // B's own decisions, as its engine reports them on the control stream.
    let decisions = timeout(Duration::from_secs(300), async {
        let mut failures: Vec<(NackVerdict, RateDecision, u8, u8)> = Vec::new();
        let mut climbed = false;
        loop {
            // One send per round. B reports status every second, so a round ends on time, not on
            // silence; 30 s covers a send, its retransmissions and the two silent ACK windows.
            a_write.write_all(send.as_bytes()).await.unwrap();
            let round_end = tokio::time::Instant::now() + Duration::from_secs(30);
            loop {
                let mut buf = String::new();
                let read = tokio::time::timeout_at(round_end, b_reader.read_line(&mut buf)).await;
                let Ok(Ok(n)) = read else { break };
                if n == 0 {
                    break;
                }
                let Ok(ControlEvent::EngineEvent {
                    event:
                        EngineEvent::OtaRateDecision {
                            from,
                            to,
                            decoded_level,
                            decision,
                            verdict,
                            ..
                        },
                }) = serde_json::from_str::<ControlEvent>(buf.trim())
                else {
                    continue;
                };
                if decoded_level.is_some() {
                    if !climbed && level_num(&to.name()) >= 8 {
                        // SL8 leaves one rung above the SL7 floor. A second step would be clamped
                        // there but is still reported as `NackStepDown`, so it still shows.
                        climbed = true;
                        reversed.store(true, Ordering::Relaxed);
                    }
                    continue;
                }
                if climbed {
                    failures.push((
                        verdict,
                        decision,
                        level_num(&from.name()),
                        level_num(&to.name()),
                    ));
                    if failures.len() >= 7 {
                        return failures;
                    }
                }
            }
        }
    })
    .await;

    pair.shutdown();
    let failures = decisions.expect("B never logged seven failures after climbing");
    let verdicts: Vec<NackVerdict> = failures.iter().map(|f| f.0).collect();
    assert_eq!(
        &verdicts[..6],
        &[
            NackVerdict::Keyed,
            NackVerdict::Keyed,
            NackVerdict::Keyed,
            NackVerdict::Muted,
            NackVerdict::Muted,
            NackVerdict::Muted
        ],
        "the budget's verdicts reach the engine: {failures:?}"
    );
    let steps = failures
        .iter()
        .filter(|f| f.1 == RateDecision::NackStepDown)
        .count();
    assert_eq!(
        steps,
        1,
        "one announced step and no silent one among {} failures: {failures:?}",
        failures.len()
    );
    assert!(
        failures[3..].iter().all(|f| f.2 == f.3),
        "a muted failure moved the recommendation: {failures:?}"
    );
}
