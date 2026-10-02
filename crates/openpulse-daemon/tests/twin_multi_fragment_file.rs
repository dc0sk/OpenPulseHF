//! A file several SAR fragments long crosses two real daemons (#1461).
//!
//! VERIFIES: REQ-FX-06 — an airtime-bounded burst of several fragments is delivered whole.
//!
//! The daemon sends a planned burst's fragments back to back inside one keying, so the receiver's
//! accumulator gathers them as ONE burst. Before `decode_burst_frames` every decode arm returned the
//! first frame of a burst and dropped the rest. Measured 2026-10-02 with this file: the 878 B file
//! (4 fragments) never arrived — B decoded one 255 B frame and reported `file_failed` `Stall` at
//! 121 s — while the one-fragment control arrived in 3.6 s.
//!
//! Default `burst_max_secs` (20 s): two 8.6 s BPSK250 fragments per keying, under the receiver's
//! burst cap. Each test asserts that some keying really carried more than one frame, so it cannot
//! pass by the planner splitting the file into one-frame keyings.

use std::time::Duration;

use openpulse_channel::awgn::AwgnChannel;
use openpulse_channel::AwgnConfig;
use openpulse_config::OpenpulseConfig;
use openpulse_core::sar::SAR_MAX_FRAGMENT_DATA;
use openpulse_daemon::protocol::{ControlCommand, ControlEvent};
use openpulse_daemon::twin::spawn_bridged_pair;
use openpulse_modem::event::EngineEvent;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;

/// Above every port the sibling daemon test binaries use (19010–19185).
const PORT_BASE: u16 = 19200;

/// Under the receiver's 120 s block stall, so a lost fragment fails here instead of being masked.
const BUDGET: Duration = Duration::from_secs(100);

fn ft_cfg(callsign: &str, port: u16, download_dir: &std::path::Path, ota: bool) -> OpenpulseConfig {
    let mut c = OpenpulseConfig::default();
    c.station.callsign = callsign.into();
    c.station.auto_id_interval_secs = 0;
    c.modem.mode = "BPSK250".into();
    c.modem.ota_enabled = ota;
    c.daemon.tcp_port = port;
    c.daemon.websocket_port = port + 1;
    c.file_transfer.enabled = true;
    c.file_transfer.require_verified_peer = false;
    c.file_transfer.auto_accept_max_bytes = 10_000_000;
    c.file_transfer.max_file_bytes = 10_000_000;
    c.file_transfer.download_dir = download_dir.to_string_lossy().into_owned();
    c
}

fn clean(seed: u64) -> Box<AwgnChannel> {
    Box::new(AwgnChannel::new(AwgnConfig::new(40.0, Some(seed))).unwrap())
}

/// Send a `len`-byte file A → B; return whether it arrived intact and A's frames per keying.
async fn send_file(len: usize, port: u16, ota: bool) -> (bool, Vec<u32>) {
    let base = std::env::temp_dir().join(format!("opfx_multifrag_{}_{port}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let recv_dir = base.join("recv");
    std::fs::create_dir_all(&base).unwrap();
    let src = base.join("payload.bin");
    // Incompressible, so session compression (on with OTA) cannot shrink it to fewer fragments.
    let mut x = 0x2545_f491_u32;
    let contents: Vec<u8> = (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect();
    std::fs::write(&src, &contents).unwrap();

    let pair = spawn_bridged_pair(
        ft_cfg("MFRA", port, &base.join("dl_a"), ota),
        ft_cfg("MFRB", port + 2, &recv_dir, ota),
        clean(1),
        clean(2),
        Duration::from_millis(10),
    )
    .await;

    let b = TcpStream::connect(pair.addr_b).await.unwrap();
    let (b_read, _b_write) = b.into_split();
    let mut b_reader = BufReader::new(b_read);
    let a = TcpStream::connect(pair.addr_a).await.unwrap();
    let (a_read, mut a_write) = a.into_split();

    // Frames A transmitted per keying, from its own control stream. A keying's `FrameTransmitted`
    // events can reach the stream after its PTT release, so each frame is credited to the most recent
    // keying rather than to an open window.
    let keyings = tokio::spawn(async move {
        let mut reader = BufReader::new(a_read);
        let mut per_keying: Vec<u32> = Vec::new();
        loop {
            let mut buf = String::new();
            match reader.read_line(&mut buf).await {
                Ok(0) | Err(_) => return per_keying,
                Ok(_) => {}
            }
            match serde_json::from_str::<ControlEvent>(buf.trim()) {
                Ok(ControlEvent::PttChanged { active: true }) => per_keying.push(0),
                Ok(ControlEvent::EngineEvent {
                    event: EngineEvent::FrameTransmitted { .. },
                }) => {
                    if let Some(n) = per_keying.last_mut() {
                        *n += 1;
                    }
                }
                _ => {}
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(200)).await;
    let cmd = serde_json::to_string(&ControlCommand::SendFile {
        to: "MFRB".into(),
        path: src.to_string_lossy().into_owned(),
    })
    .unwrap()
        + "\n";
    a_write.write_all(cmd.as_bytes()).await.unwrap();

    let received = timeout(BUDGET, async {
        loop {
            let mut buf = String::new();
            if b_reader.read_line(&mut buf).await.unwrap() == 0 {
                continue;
            }
            match serde_json::from_str::<ControlEvent>(buf.trim()) {
                Ok(ControlEvent::FileReceived { path, .. }) => return Some(path),
                Ok(ControlEvent::FileFailed { .. }) => return None,
                _ => {}
            }
        }
    })
    .await
    .ok()
    .flatten();
    pair.shutdown();
    let per_keying = keyings.await.unwrap_or_default();
    let intact = received.is_some_and(|p| std::fs::read(p).ok().as_deref() == Some(&contents[..]));
    let _ = std::fs::remove_dir_all(&base);
    (intact, per_keying)
}

/// Four fragments: one block, 3.5 fragments of data plus the block header.
const MULTI: usize = SAR_MAX_FRAGMENT_DATA * 7 / 2;

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_multi_fragment_file_crosses_two_daemons() {
    let (intact, per_keying) = send_file(MULTI, PORT_BASE, false).await;
    assert!(
        per_keying.iter().any(|&n| n > 1),
        "no keying carried more than one frame ({per_keying:?}) — the test would be vacuous"
    );
    assert!(
        intact,
        "the {MULTI} B file did not arrive intact within {BUDGET:?}; frames per keying on A: \
         {per_keying:?}"
    );
}

/// The same through the OTA arm's uncoded fallback, the daemon's on-air configuration.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_multi_fragment_file_crosses_two_daemons_with_ota_enabled() {
    let (intact, per_keying) = send_file(MULTI, PORT_BASE + 4, true).await;
    assert!(
        per_keying.iter().any(|&n| n > 1),
        "no keying carried more than one frame ({per_keying:?}) — the test would be vacuous"
    );
    assert!(
        intact,
        "the {MULTI} B file did not arrive intact with OTA on; frames per keying on A: \
         {per_keying:?}"
    );
}

/// Control: one fragment, the case that always worked.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn control_a_one_fragment_file_crosses_two_daemons() {
    let (intact, per_keying) = send_file(SAR_MAX_FRAGMENT_DATA / 2, PORT_BASE + 8, false).await;
    assert!(intact, "frames per keying on A: {per_keying:?}");
}
