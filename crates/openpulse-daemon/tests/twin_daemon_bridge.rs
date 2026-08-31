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

/// Hand out a control-port pair that no other test in this binary can be using.
///
/// **Ports were assigned by hand and collided twice.** `PTTA/PTTB` and `OFFSETA/OFFSETB` were both
/// given 19060–19063; libtest runs these tests CONCURRENTLY in one process, so when their windows
/// overlapped the second daemon to bind got `Address already in use`, `run()` returned `Err`, and
/// that daemon exited — which presented as the #1176 intermittent "daemon B never decoded a frame".
/// It was invisible to a filtered run, because a filter never starts the colliding test. An earlier
/// occurrence had picked 19040–19043, already used by `subfloor_cfg`.
///
/// Allocating removes the class rather than the instance: a third careful hand-assignment would
/// only be waiting for the fourth collision.
fn alloc_ports() -> (u16, u16) {
    use std::sync::atomic::{AtomicU16, Ordering};
    // This binary's window. Sibling daemon test BINARIES hand-pick ports above it
    // (`ota_ack_capture_stream` 19140–19143, `monitor_during_ota` 19160–19161) and run as separate
    // processes concurrently under `cargo test`, so the cross-binary collision is real even though
    // this allocator makes the within-binary one impossible. Fail LOUDLY on outgrowing the window
    // rather than silently wandering into a sibling's ports and reintroducing #1176 one binary over.
    const BASE: u16 = 19010;
    const CEILING: u16 = 19140;
    static NEXT: AtomicU16 = AtomicU16::new(BASE);
    let base = NEXT.fetch_add(2, Ordering::Relaxed);
    assert!(
        base + 1 < CEILING,
        "twin tests have outgrown their port window {BASE}..{CEILING}; the next allocation would \
         reach {base}, which collides with another daemon test binary. Move this window rather \
         than letting it overlap — that is exactly how #1176 happened."
    );
    (base, base + 1)
}

fn cfg_auto(callsign: &str) -> OpenpulseConfig {
    let (tcp, ws) = alloc_ports();
    cfg(callsign, tcp, ws)
}

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

/// A clean channel with a **carrier offset** — the impairment two real rigs always have.
///
/// Composed rather than replacing the AWGN arm: the offset is the variable under test, and dropping
/// the noise would make the test easier than the control it is compared against.
struct OffsetChannel {
    cfo: openpulse_channel::cfo::CfoChannel,
    awgn: AwgnChannel,
}

impl openpulse_channel::ChannelModel for OffsetChannel {
    fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        let shifted = self.cfo.apply(input);
        self.awgn.apply(&shifted)
    }
    fn generate_noise(&mut self, length: usize) -> Vec<f32> {
        self.awgn.generate_noise(length)
    }
}

fn offset_channel(seed: u64, offset_hz: f32) -> Box<OffsetChannel> {
    Box::new(OffsetChannel {
        cfo: openpulse_channel::cfo::CfoChannel::new(openpulse_channel::cfo::CfoConfig::new(
            offset_hz, 8_000.0,
        ))
        .expect("finite offset"),
        awgn: AwgnChannel::new(AwgnConfig::new(40.0, Some(seed))).unwrap(),
    })
}

/// Parse a `"SLn"` level name to its number (e.g. `"SL4"` → 4); 0 if unparseable.
fn level_num(name: &str) -> u8 {
    name.trim_start_matches("SL").parse().unwrap_or(0)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn message_crosses_the_bridge_between_two_real_daemons() {
    let pair = spawn_bridged_pair(
        cfg_auto("DAEMONA"),
        cfg_auto("DAEMONB"),
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

fn ota_cfg(callsign: &str) -> OpenpulseConfig {
    let mut c = cfg_auto(callsign);
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
        ota_cfg("OTAA"),
        ota_cfg("OTAB"),
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
    // Generous outer budget: this drives 10 real two-daemon OTA sends (~18 s isolated) and must not
    // spuriously time out when the full suite runs it under concurrent CPU load — the assertion is on
    // the ladder stepping, not on latency (audit finding S4-3).
    let max_level = timeout(Duration::from_secs(120), async {
        let mut max_seen = 2u8; // SL2 floor
        for _ in 0..10 {
            a_write.write_all(send.as_bytes()).await.unwrap();
            // Read until the post-send OtaStatus (or any) reports a tx_level.
            let round = timeout(Duration::from_secs(11), async {
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
fn ft_cfg(callsign: &str, download_dir: &std::path::Path) -> OpenpulseConfig {
    let mut c = cfg_auto(callsign);
    c.file_transfer.enabled = true;
    c.file_transfer.require_verified_peer = false;
    c.file_transfer.auto_accept_max_bytes = 10_000_000;
    c.file_transfer.max_file_bytes = 10_000_000;
    c.file_transfer.download_dir = download_dir.to_string_lossy().into_owned();
    c
}

/// FF-16 Phase C acceptance: a file sent from daemon A lands, reassembled byte-for-byte, on daemon B —
/// across the real modem + a clean channel, driven entirely through the control protocol.
///
/// VERIFIES: REQ-FX-07 — block-level progress surfaced to the operator at BOTH ends, asserted on
/// the two daemons' real control streams.
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
        ft_cfg("STNA", &base.join("dl_a")),
        ft_cfg("STNB", &recv_dir),
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
    let (a_read, mut a_write) = a.into_split();

    // REQ-FX-07's "at both ends" clause: the SENDER's operator stream must carry tx-direction
    // progress. Collected concurrently — this stream is live only while the transfer runs.
    let tx_progress = tokio::spawn(async move {
        let mut reader = BufReader::new(a_read);
        let mut seen: Vec<(u16, u16)> = Vec::new();
        loop {
            let mut buf = String::new();
            match reader.read_line(&mut buf).await {
                Ok(0) | Err(_) => return seen,
                Ok(_) => {}
            }
            if let Ok(ControlEvent::FileProgress {
                direction,
                blocks_done,
                blocks_total,
                ..
            }) = serde_json::from_str::<ControlEvent>(buf.trim())
            {
                if direction == "tx" {
                    seen.push((blocks_done, blocks_total));
                }
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(200)).await;
    let cmd = serde_json::to_string(&ControlCommand::SendFile {
        to: "STNB".into(),
        path: src.to_string_lossy().into_owned(),
    })
    .unwrap()
        + "\n";
    a_write.write_all(cmd.as_bytes()).await.unwrap();

    // Daemon B must emit FileReceived; capture the path it wrote to, and the rx-direction
    // progress that preceded it (REQ-FX-07, receiving end).
    let mut rx_progress: Vec<(u16, u16)> = Vec::new();
    let received = timeout(Duration::from_secs(90), async {
        loop {
            let mut buf = String::new();
            if b_reader.read_line(&mut buf).await.unwrap() == 0 {
                continue;
            }
            match serde_json::from_str::<ControlEvent>(buf.trim()) {
                Ok(ControlEvent::FileReceived { path, name, .. }) => return (path, name),
                Ok(ControlEvent::FileProgress {
                    direction,
                    blocks_done,
                    blocks_total,
                    ..
                }) if direction == "rx" => rx_progress.push((blocks_done, blocks_total)),
                _ => {}
            }
        }
    })
    .await;

    pair.shutdown();

    let (path, name) = received.expect("daemon B never reported the file crossing the bridge");
    assert!(name.contains("payload"), "unexpected file name {name}");
    let got = std::fs::read(&path).expect("received file must exist on disk");
    assert_eq!(got, contents, "reassembled file must match the sent bytes");

    // REQ-FX-07: progress must reach the OPERATOR at both ends, not merely be emitted by the
    // state machines. Bound here rather than on the sender unit test, whose assertions survive
    // deleting the daemon's forwarding entirely.
    let tx_progress = tx_progress.await.unwrap_or_default();
    assert!(
        !tx_progress.is_empty(),
        "sending daemon never surfaced FileProgress to its control stream"
    );
    assert!(
        !rx_progress.is_empty(),
        "receiving daemon never surfaced FileProgress to its control stream"
    );
    for (done, total) in tx_progress.iter().chain(rx_progress.iter()) {
        assert!(
            *total > 0 && done <= total,
            "nonsensical progress {done}/{total}"
        );
    }
    // Deliberately NOT asserted: that the sender's LAST progress reads blocks_done == total.
    // The terminal `progress(block_count)` (filexfer sender.rs:93,116) fires on completion, which
    // races `pair.shutdown()` closing this stream — a load-dependent verdict of the kind #1066
    // exists to remove. The requirement is that progress reaches the operator, not its final value.

    let _ = std::fs::remove_dir_all(&base);
}

/// Config for a station pinned at the MFSK16 SL1 sub-floor rung on the `hpx_hf` profile.
fn subfloor_cfg(callsign: &str) -> OpenpulseConfig {
    let mut c = cfg_auto(callsign);
    c.modem.ota_enabled = true;
    c.modem.ota_profile = "hpx_hf".into(); // has SL1 = MFSK16
    c.modem.ota_lock_level = "SL1".into(); // pin at the sub-floor rung
    c
}

/// End-to-end validation of the MFSK16 sub-floor ARQ rung across two REAL daemons (REQ-WSIG-01):
/// both pinned at SL1, daemon A sends a small message; daemon B must decode the MFSK16 data frame
/// (`FrameReceived` with `mode == "MFSK16"`) and answer with a K=3 MFSK16-ACK that A recovers by
/// union-listening — A's `OtaStatus` (emitted only after `apply_ota_ack`, reporting `tx_mode == "MFSK16"`)
/// confirms the ACK completed the exchange on the sub-floor rung, not a fallback.
///
/// The 17 s MFSK16 frame is 17 s of *audio samples*, processed at CPU speed over the (non-real-time)
/// loopback bridge — the whole exchange runs in ~1 s, so this is a routine CI gate. (The *dynamic*
/// entry+exit boundary — a deep fade dropping the live ladder to SL1 then recovering — needs mid-test
/// channel control and remains deferred.)
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn subfloor_sl1_message_crosses_with_k3_ack_between_two_real_daemons() {
    let pair = spawn_bridged_pair(
        subfloor_cfg("SUBA"),
        subfloor_cfg("SUBB"),
        clean_awgn(3),
        clean_awgn(4),
        Duration::from_millis(10),
    )
    .await;

    let b = TcpStream::connect(pair.addr_b).await.unwrap();
    let (b_read, _bw) = b.into_split();
    let mut b_reader = BufReader::new(b_read);
    let a = TcpStream::connect(pair.addr_a).await.unwrap();
    let (a_read, mut a_write) = a.into_split();
    let mut a_reader = BufReader::new(a_read);

    tokio::time::sleep(Duration::from_millis(250)).await;
    let cmd = serde_json::to_string(&ControlCommand::SendMessage {
        to: "SUBB".into(),
        subject: "s".into(),
        body: "sub-floor arq hello".into(), // ≤ 209 B → fits one MFSK16 frame
    })
    .unwrap()
        + "\n";
    a_write.write_all(cmd.as_bytes()).await.unwrap();

    // B decodes the MFSK16 data frame (capture its mode); A receives B's K=3 MFSK16-ACK (its OtaStatus
    // reports tx_mode). Watch both concurrently, and assert the exchange ACTUALLY used the sub-floor rung —
    // on a clean loopback the ladder could otherwise decode via a different candidate and pass trivially.
    let (b_mode, a_tx_mode) = tokio::join!(
        timeout(Duration::from_secs(120), async {
            loop {
                let mut buf = String::new();
                if b_reader.read_line(&mut buf).await.unwrap() == 0 {
                    continue;
                }
                if let Ok(ControlEvent::EngineEvent {
                    event: openpulse_modem::EngineEvent::FrameReceived { mode, bytes },
                }) = serde_json::from_str::<ControlEvent>(buf.trim())
                {
                    if bytes > 0 {
                        return mode;
                    }
                }
            }
        }),
        timeout(Duration::from_secs(120), async {
            loop {
                let mut buf = String::new();
                if a_reader.read_line(&mut buf).await.unwrap() == 0 {
                    continue;
                }
                if let Ok(ControlEvent::OtaStatus { tx_mode, .. }) =
                    serde_json::from_str::<ControlEvent>(buf.trim())
                {
                    return tx_mode;
                }
            }
        }),
    );

    pair.shutdown();
    let b_mode = b_mode.expect("daemon B never decoded the frame daemon A transmitted");
    let a_tx_mode = a_tx_mode.expect("daemon A never got an OtaStatus (no ACK applied)");
    assert_eq!(
        b_mode, "MFSK16",
        "B must decode the sub-floor frame as MFSK16, not a fallback rung"
    );
    assert_eq!(
        a_tx_mode.as_deref(),
        Some("MFSK16"),
        "A must be transmitting at the pinned MFSK16 SL1 rung (lock took effect)"
    );
}

/// The same file transfer, with the ONE config change `[modem] ota_enabled = true`.
///
/// `a_file_crosses_the_bridge_between_two_real_daemons` above runs with OTA off, which is the
/// default — so nothing in the suite exercised filexfer under the configuration the on-air campaign
/// actually uses. `server::run`'s rx_ticker dispatches a flushed burst to `ota_decode_burst`
/// whenever a session is active (and `start_ota_session` runs once at startup and is never cleared
/// except by an explicit `StopOtaSession`), and that arm tries only the current rung's candidates —
/// at most two `(mode, FEC)` pairs. `drain_filexfer_tx` transmits fragments with `engine.transmit`
/// — uncoded. Under the default `hpx_hf` every rung is coded, so no candidate can match.
///
/// The arm is isolated as the single cause in
/// `openpulse-modem/tests/ota_arm_uncoded_dispatch.rs`, which decodes ONE burst both ways with the
/// candidate mode locked to the transmitted one; this test is the consequence at the shipping
/// surface. Its control is the OTA-off test above: same file, same channel, same code path,
/// differing only in `ota_enabled`.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_file_crosses_the_bridge_with_ota_enabled() {
    let base = std::env::temp_dir().join(format!("opfx_twin_ota_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let recv_dir = base.join("recv");
    std::fs::create_dir_all(&base).unwrap();

    let src = base.join("payload.txt");
    let contents = b"twin file-transfer payload across two real daemons ".repeat(4);
    std::fs::write(&src, &contents).unwrap();

    // EXACTLY one line differs from the control above. `ota_profile` is deliberately left empty so
    // the daemon falls back to `[modem] profile` (server.rs:229-233) — the default `hpx_hf`, every
    // rung of which is coded. Setting `ota_profile = "hpx500"` here (as the first draft did) would
    // have changed the mechanism under test: `hpx500` populates no FEC table, so `fec_for` returns
    // `FecMode::None` for every rung (profile.rs:110) and the failure would have been candidate-MODE
    // mismatch rather than the absence of an uncoded candidate.
    let ota_ft = |call: &str, dir: &std::path::Path| {
        let mut c = ft_cfg(call, dir);
        c.modem.ota_enabled = true;
        c
    };

    let pair = spawn_bridged_pair(
        ota_ft("STNA", &base.join("dl_a")),
        ota_ft("STNB", &recv_dir),
        clean_awgn(1),
        clean_awgn(2),
        Duration::from_millis(10),
    )
    .await;

    let b = TcpStream::connect(pair.addr_b).await.unwrap();
    let (b_read, _b_write) = b.into_split();
    let mut b_reader = BufReader::new(b_read);

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

    let (path, name) = received
        .expect("daemon B never reported the file crossing the bridge with ota_enabled = true");
    assert!(name.contains("payload"), "unexpected file name {name}");
    let got = std::fs::read(&path).expect("received file must exist on disk");
    assert_eq!(got, contents, "reassembled file must match the sent bytes");
    let _ = std::fs::remove_dir_all(&base);
}

/// An OTA-enabled receiver must not KEY THE TRANSMITTER at a peer's non-ladder traffic.
///
/// The reception half of #1123 is gated above; this is the controller/PTT half at the shipping
/// surface. On `main` a heard uncoded frame counted as a decode failure, so it drove
/// `on_rx_frame(RxOutcome::Failed, ..)` and — within `OTA_NACK_BUDGET` — keyed a NACK back at the
/// sender. The fix gates the whole keying block on `ladder_frame` (`res.ack.is_some()`), and without
/// this test that gate is verified only by reading.
///
/// A is a plain (non-OTA) station sending one uncoded message; B runs an OTA session. B has nothing
/// to say in reply — no ladder frame, no ACK, and `SendMessage` on A is the only traffic — so the
/// expected `PttChanged` count on B is exactly **zero**. That makes the bound exact rather than a
/// tuned threshold.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn an_ota_receiver_does_not_key_ptt_at_a_peers_uncoded_traffic() {
    let mut b_cfg = cfg_auto("PTTB");
    b_cfg.modem.ota_enabled = true;
    let pair = spawn_bridged_pair(
        cfg_auto("PTTA"),
        b_cfg,
        clean_awgn(1),
        clean_awgn(2),
        Duration::from_millis(10),
    )
    .await;

    let b = TcpStream::connect(pair.addr_b).await.unwrap();
    let (b_read, _b_write) = b.into_split();
    let mut b_reader = BufReader::new(b_read);

    let a = TcpStream::connect(pair.addr_a).await.unwrap();
    let (_a_read, mut a_write) = a.into_split();
    tokio::time::sleep(Duration::from_millis(150)).await;
    let cmd = serde_json::to_string(&ControlCommand::SendMessage {
        to: "PTTB".into(),
        subject: "x".into(),
        body: "uncoded control traffic".into(),
    })
    .unwrap()
        + "\n";
    a_write.write_all(cmd.as_bytes()).await.unwrap();

    // Watch B long enough to cover several receive ticks and the whole NACK budget.
    let keyed = timeout(Duration::from_secs(12), async {
        let mut keyed = 0usize;
        loop {
            let mut buf = String::new();
            if b_reader.read_line(&mut buf).await.unwrap() == 0 {
                continue;
            }
            if let Ok(ControlEvent::PttChanged { active }) =
                serde_json::from_str::<ControlEvent>(buf.trim())
            {
                if active {
                    keyed += 1;
                    return keyed;
                }
            }
        }
    })
    .await;

    pair.shutdown();
    assert!(
        keyed.is_err(),
        "an OTA-enabled receiver keyed PTT at a peer's uncoded traffic ({keyed:?} assertions) — \
         that is the NACK-at-your-peer's-file-transfer defect #1123 closed"
    );
}

/// Two real daemons whose rigs disagree on frequency still exchange traffic (#1118).
///
/// **What this does NOT gate, established by sabotage rather than assumed.** Disabling the #1118
/// acquisition pass entirely leaves this test **passing** — at −64 Hz, at −200 Hz and at −400 Hz.
/// This path (a plain, uncoded `SendMessage` over a 40 dB AWGN bridge with no lead-in noise)
/// tolerates large offsets natively, so it cannot detect the defect and must not be cited as the
/// gate for the fix. That role belongs to
/// `openpulse-modem/tests/daemon_frequency_acquisition.rs`, whose REQ-PHY-03 gate goes red when the
/// pass is disabled.
///
/// It is kept, and it is not vacuous: at **−800 Hz** — past `AFC_MAX_CORRECTION_HZ` — it fails, so it
/// does measure the offset it applies. What it covers is a **one-way** message crossing two real
/// daemons at a realistic inter-rig offset: the control protocol, the transmit path, the bridge, and
/// B's capture and decode.
///
/// **Corrected 2026-08-21 (#1177): it is NOT a round trip and does NOT reach the ISS ACK listen.**
/// This comment claimed both, and claimed `receive_ota_ack_within` "has no other test" — which made
/// the false claim load-bearing, since it was the stated reason to keep the test. At this test's
/// config `modem.ota_enabled` is the default `false` (`openpulse-config/src/lib.rs:660`),
/// `start_ota_session` is gated on it (`server.rs:228`), and the non-OTA `SendMessage` arm is a
/// one-way fixed-mode transmit (`lib.rs:2031-2050`). B never keys; the reverse-offset channel this
/// test installs is dead weight. **The FSK4-ACK-at-offset chain is covered by nothing**, which is
/// the real finding — FSK4-ACK tone spacing is 100 Hz, so a ±50 Hz error is catastrophic there.
/// Enabling OTA here would give that chain its first coverage, but that path carries its own
/// wall-clock deadlines (`engine.rs:6023`, `:6189`) and must not be added while #1176 is open.
///
/// −64 Hz is the one cleanly measured inter-rig offset on this project's hardware (IC-9700 <->
/// FT-991A, both commanded to 144.600000 MHz, 2026-07-28; `openpulse-channel/src/cfo.rs`), and it is
/// already past `REQ-PHY-03`'s ±50 Hz requirement. The offset is applied in BOTH directions, because
/// two rigs that disagree disagree symmetrically.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_message_crosses_the_bridge_between_two_rigs_that_disagree_on_frequency() {
    const MEASURED_INTER_RIG_OFFSET_HZ: f32 = -64.0;

    let pair = spawn_bridged_pair(
        cfg_auto("OFFSETA"),
        cfg_auto("OFFSETB"),
        offset_channel(1, MEASURED_INTER_RIG_OFFSET_HZ),
        offset_channel(2, MEASURED_INTER_RIG_OFFSET_HZ),
        Duration::from_millis(10),
    )
    .await;

    let b = TcpStream::connect(pair.addr_b).await.unwrap();
    let (b_read, _b_write) = b.into_split();
    let mut b_reader = BufReader::new(b_read);

    let a = TcpStream::connect(pair.addr_a).await.unwrap();
    let (a_read, mut a_write) = a.into_split();
    let mut a_reader = BufReader::new(a_read);

    // A's stream is watched too. A transmitter that never keyed looks, from B, exactly like a
    // receiver that never heard — and nothing in this rig could tell them apart: the test never
    // read A, the daemon's `tracing::error!` on exit goes nowhere (no subscriber is installed),
    // and `shutdown()` discarded join results.
    let a_events = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let a_sink = a_events.clone();
    let a_watch = tokio::spawn(async move {
        loop {
            let mut buf = String::new();
            match a_reader.read_line(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let line = buf.trim().to_string();
            if !line.is_empty() {
                if let Ok(mut v) = a_sink.lock() {
                    if v.len() < 4_000 {
                        v.push(line);
                    }
                }
            }
        }
    });

    // Both control stacks must be live and subscribed BEFORE the command, or an event lost to a
    // late subscription is indistinguishable from a frame that never crossed. A fixed sleep
    // asserts nothing: TCP connect succeeds on the listen backlog without the acceptor running,
    // and a broadcast subscriber never sees what was published before it subscribed.
    let synced = timeout(Duration::from_secs(30), async {
        loop {
            let mut buf = String::new();
            match b_reader.read_line(&mut buf).await {
                Ok(0) | Err(_) => return false,
                Ok(_) => {}
            }
            if !buf.trim().is_empty() {
                return true;
            }
        }
    })
    .await;
    assert!(
        matches!(synced, Ok(true)),
        "daemon B published no control event within 30 s of connecting, so its control stack was \
         never live and nothing measured below would be attributable"
    );

    let cmd = serde_json::to_string(&ControlCommand::SendMessage {
        to: "OFFSETB".into(),
        subject: "x".into(),
        body: "offset round trip".into(),
    })
    .unwrap()
        + "\n";
    a_write.write_all(cmd.as_bytes()).await.unwrap();

    // This verdict is WALL-CLOCK bounded by construction — it is an end-to-end round trip between
    // two real daemons whose receive ticks run in real time, so there is no work counter to bound it
    // on the way #1066 bounded the receive search. The bound is therefore set from the MEASURED idle
    // cost with a large multiplier: this test completes in ~4.4 s on an idle machine, and it failed
    // at 60 s inside a full `gate.sh` run, where every core is busy. 300 s is ~68x idle.
    //
    // A timeout alone cannot say WHICH world it is in, so the loop counts what arrived: zero control
    // events means the daemons were starved and the machine is the story, while events-without-a-
    // decode is the real failure this gate exists to catch.
    let mut events_seen = 0usize;
    let mut b_kinds: std::collections::BTreeMap<String, usize> = Default::default();
    let got = timeout(Duration::from_secs(300), async {
        loop {
            let mut buf = String::new();
            if b_reader.read_line(&mut buf).await.unwrap() == 0 {
                continue;
            }
            let line = buf.trim();
            if line.is_empty() {
                continue;
            }
            events_seen += 1;
            *b_kinds
                .entry(
                    serde_json::from_str::<serde_json::Value>(line)
                        .ok()
                        .and_then(|v| {
                            v.get("event")
                                .and_then(|e| e.get("type"))
                                .and_then(|t| t.as_str())
                                .map(|t| format!("engine:{t}"))
                                .or_else(|| {
                                    v.get("type").and_then(|t| t.as_str()).map(str::to_string)
                                })
                        })
                        .unwrap_or_else(|| "unparsed".into()),
                )
                .or_default() += 1;
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

    // Snapshot the rig BEFORE shutdown: `stats()` reads liveness from the daemon threads, so
    // shutting down first would make every failure report "EXITED".
    let stats = pair.stats();
    let a_lines = a_events.lock().map(|v| v.clone()).unwrap_or_default();
    a_watch.abort();
    pair.shutdown();

    // A raw count cannot say what failed: both daemons publish a 2 Hz heartbeat regardless, so
    // "600 events in 300 s" is exactly what a completely idle receive path looks like. Tally kinds.
    let kind_of = |line: &String| -> String {
        serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .map(|v| match v.get("event").and_then(|e| e.get("type")) {
                Some(t) => format!("engine:{}", t.as_str().unwrap_or("?")),
                None => v
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("?")
                    .to_string(),
            })
            .unwrap_or_else(|| "unparsed".into())
    };
    let mut a_kinds: std::collections::BTreeMap<String, usize> = Default::default();
    for l in &a_lines {
        *a_kinds.entry(kind_of(l)).or_default() += 1;
    }
    let a_len = a_lines.len();

    assert!(
        got.is_ok(),
        "daemon B never decoded a frame across a -64 Hz inter-rig offset in 300 s.\n\
         B: {events_seen} events, kinds {b_kinds:?}\n\
         A: {a_len} events, kinds {a_kinds:?}\n\
         rig: {stats}\n\
         REQ-PHY-03 requires tracking station-to-station offsets to +/-50 Hz without operator \
         intervention, and this is the shipping two-daemon surface. Read it in this order: no \
         transmit event on A, or a dead A, puts the fault upstream of the channel; forward samples \
         ~0 means the bridge; samples moved with no DcdChange on B means B's capture seam; a \
         DcdChange without a decode is the acquisition failure this gate is actually for."
    );
}
