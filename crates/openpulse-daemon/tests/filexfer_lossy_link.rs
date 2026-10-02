//! VERIFIES: REQ-FX-05 — selective retransmission recovers lost fragments and acks.
//!
//! File transfer between two stations' real file-transfer code over a link that loses chosen frames
//! (selective-repeat design: `docs/dev/design/filexfer-selective-repeat.md`).
//!
//! Each station is a `RuntimeControlState` driven through the daemon's own entry points:
//! `send_file`, `route_inbound_fragment` (what the receive tick calls for every decoded fragment),
//! `poll_timeouts` and `note_round_sent` (what `drain_filexfer_tx` calls once a round is on the air).
//! A "drain" here hands each queued SAR fragment to the peer unless the loss predicate drops it. The
//! modem is not in this loop on purpose: these are protocol losses, injected exactly and in virtual
//! time; `twin_multi_fragment_file` covers the same traffic through two real daemons and the modem.

use std::sync::Arc;

use openpulse_daemon::filexfer::{
    note_round_sent, poll_timeouts, route_inbound_fragment, send_file, FileTransferPolicy,
};
use openpulse_daemon::protocol::ControlEvent;
use openpulse_daemon::RuntimeControlState;
use openpulse_filexfer::{FxFrame, Timeouts};
use tokio::sync::broadcast;

const MODE: &str = "BPSK250";

struct Station {
    rs: RuntimeControlState,
    ev: Arc<broadcast::Sender<ControlEvent>>,
    rx: broadcast::Receiver<ControlEvent>,
}

fn station(call: &str, dir: &std::path::Path, seed: u8) -> Station {
    let (tx, rx) = broadcast::channel(4096);
    let mut policy = FileTransferPolicy::default();
    policy.offer.enabled = true;
    policy.offer.require_verified_peer = false;
    policy.offer.auto_accept_max_bytes = 10_000_000;
    policy.offer.max_file_bytes = 10_000_000;
    policy.download_dir = dir.to_path_buf();
    let mut s = [0u8; 32];
    s[0] = seed;
    Station {
        rs: RuntimeControlState {
            local_callsign: call.into(),
            station_seed: s,
            filexfer_policy: policy,
            ..RuntimeControlState::default()
        },
        ev: Arc::new(tx),
        rx,
    }
}

/// The decoded control frame a SAR fragment carries, when it is a single-fragment control frame.
fn ctrl(frag: &[u8]) -> Option<FxFrame> {
    let segment = u16::from_be_bytes([frag[0], frag[1]]);
    (segment == 0xFFFF && frag[3] == 1)
        .then(|| FxFrame::decode(&frag[4..]).ok())
        .flatten()
}

fn is_ack(frag: &[u8], block: u16, complete: bool) -> bool {
    matches!(ctrl(frag), Some(FxFrame::BlockAck { block_index, complete: c, .. })
        if block_index == block && c == complete)
}

/// Block data fragment `(block, index)`, from its SAR header.
fn data(frag: &[u8]) -> Option<(u16, u8)> {
    let segment = u16::from_be_bytes([frag[0], frag[1]]);
    (segment != 0xFFFF && segment != 0).then(|| (segment - 1, frag[2]))
}

/// Move `from`'s queued frames to `to`, dropping those `lose` selects. Returns how many were sent.
fn drain(
    from: &mut Station,
    to: &mut Station,
    now: u64,
    lose: &mut dyn FnMut(&[u8]) -> bool,
) -> usize {
    let queue = std::mem::take(&mut from.rs.filexfer_tx_queue);
    for (frag, _) in &queue {
        if !lose(frag) {
            let segment = u16::from_be_bytes([frag[0], frag[1]]);
            route_inbound_fragment(frag, segment, &mut to.rs, &to.ev, MODE);
        }
    }
    note_round_sent(&mut from.rs, now, 0);
    queue.len()
}

struct Run {
    received: Option<Vec<u8>>,
    /// The sender reported `FileSent` — success, not merely a stopped session.
    sent: bool,
    probes: usize,
}

/// Send `file` A → B over a link where `lose_ab` / `lose_ba` drop frames; run until both ends are
/// done or the virtual clock passes the receiver's stall.
fn transfer(
    file: &[u8],
    lose_ab: &mut dyn FnMut(&[u8]) -> bool,
    lose_ba: &mut dyn FnMut(&[u8]) -> bool,
) -> Run {
    static RUN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = RUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let base = std::env::temp_dir().join(format!("opfx_lossy_{}_{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let src = base.join("payload.bin");
    std::fs::write(&src, file).unwrap();
    let mut a = station("AA1AA", &base.join("a"), 1);
    let mut b = station("BB1BB", &base.join("b"), 2);
    let t = Timeouts::default();
    let start = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let mut now = start;

    send_file("BB1BB", src.to_str().unwrap(), &mut a.rs, &a.ev, MODE);
    let (mut received, mut sent, mut probes) = (None, false, 0);
    while now < start + t.block_stall_ms {
        let moved = drain(&mut a, &mut b, now, lose_ab) + drain(&mut b, &mut a, now, lose_ba);
        while let Ok(ev) = b.rx.try_recv() {
            if let ControlEvent::FileReceived { path, .. } = ev {
                received = std::fs::read(path).ok();
            }
        }
        while let Ok(ev) = a.rx.try_recv() {
            sent |= matches!(ev, ControlEvent::FileSent { .. });
        }
        if a.rs.file_tx.is_none() && b.rs.file_rx.is_none() {
            break;
        }
        if moved == 0 {
            // The link is idle: let the clock run to the sender's next deadline.
            now += t.ack_wait_ms;
            poll_timeouts(&mut a.rs, &a.ev, MODE, now);
            poll_timeouts(&mut b.rs, &b.ev, MODE, now);
            probes += usize::from(!a.rs.filexfer_tx_queue.is_empty());
        }
    }
    let _ = std::fs::remove_dir_all(&base);
    Run {
        received,
        sent,
        probes,
    }
}

/// 20 000 incompressible bytes: two 16 KiB blocks, 66 + 15 fragments.
fn file() -> Vec<u8> {
    let mut x = 0x9e37_79b9_u32;
    (0..20_000)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

fn once(mut pick: impl FnMut(&[u8]) -> bool) -> impl FnMut(&[u8]) -> bool {
    let mut done = false;
    move |f| {
        if !done && pick(f) {
            done = true;
            return true;
        }
        false
    }
}

fn keep(_: &[u8]) -> bool {
    false
}

fn assert_delivered(run: &Run, file: &[u8], what: &str) {
    assert_eq!(
        run.received.as_deref(),
        Some(file),
        "{what}: the file must arrive intact"
    );
    assert!(
        run.sent,
        "{what}: the sender must report success, not stall"
    );
}

#[test]
fn control_a_clean_link_needs_no_probe() {
    let f = file();
    let run = transfer(&f, &mut keep, &mut keep);
    assert_delivered(&run, &f, "clean link");
    assert_eq!(run.probes, 0);
}

#[test]
fn a_lost_fragment_is_recovered_by_a_nack() {
    let f = file();
    let run = transfer(&f, &mut once(|x| data(x) == Some((0, 7))), &mut keep);
    assert_delivered(&run, &f, "lost fragment");
    assert_eq!(
        run.probes, 0,
        "the NACK at the end of the round needs no probe"
    );
}

#[test]
fn a_lost_block_ack_is_recovered_by_a_probe() {
    let f = file();
    let run = transfer(&f, &mut keep, &mut once(|x| is_ack(x, 0, true)));
    assert_delivered(&run, &f, "lost BlockAck");
    assert_eq!(run.probes, 1);
}

#[test]
fn a_lost_last_fragment_of_the_round_is_recovered_by_a_probe() {
    let f = file();
    let run = transfer(&f, &mut once(|x| data(x) == Some((0, 65))), &mut keep);
    assert_delivered(&run, &f, "lost round end");
    assert_eq!(run.probes, 1);
}

#[test]
fn a_lost_nack_is_answered_again_on_the_probe() {
    let f = file();
    let run = transfer(
        &f,
        &mut once(|x| data(x) == Some((0, 7))),
        &mut once(|x| is_ack(x, 0, false)),
    );
    assert_delivered(&run, &f, "lost NACK");
    assert_eq!(run.probes, 1);
}

#[test]
fn a_lost_ack_and_file_complete_on_the_last_block_still_end_in_success() {
    let f = file();
    let mut lost = 0;
    let mut lose = |x: &[u8]| {
        let hit = lost < 2
            && (is_ack(x, 1, true) || matches!(ctrl(x), Some(FxFrame::FileComplete { .. })));
        lost += usize::from(hit);
        hit
    };
    let run = transfer(&f, &mut keep, &mut lose);
    assert_delivered(&run, &f, "lost final ack and FileComplete");
    assert_eq!(run.probes, 1, "one probe draws the re-sent FileComplete");
}

/// The deadlock the review found: a NACK lost after a RESEND round. B asked for {5, 9}, got 9 but lost
/// 5 again, NACKed {5} (its round now ends at 5) and that NACK was lost. A's probe re-sends 9, the end
/// of A's round — not B's. Only the duplicate rule answers it; without it A probes until it stalls.
#[test]
fn a_lost_nack_after_a_resend_round_is_answered_on_the_duplicate() {
    let f = file();
    let (mut fives, mut nines, mut nacks) = (0, 0, 0);
    let mut lose_ab = |x: &[u8]| match data(x) {
        Some((0, 5)) => {
            fives += 1;
            fives <= 2
        }
        Some((0, 9)) => {
            nines += 1;
            nines == 1
        }
        _ => false,
    };
    let mut lose_ba = |x: &[u8]| {
        if is_ack(x, 0, false) {
            nacks += 1;
            return nacks == 2;
        }
        false
    };
    let run = transfer(&f, &mut lose_ab, &mut lose_ba);
    assert_delivered(&run, &f, "lost NACK after a resend round");
    assert_eq!(run.probes, 1);
}
