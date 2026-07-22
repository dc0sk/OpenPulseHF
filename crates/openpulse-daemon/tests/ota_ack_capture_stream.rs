//! The daemon never holds two capture streams open on one device.
//!
//! **The defect this pins** (loose-ends audit #917, finding #6). The daemon's receive tick holds one
//! capture stream open across ticks, because cpal is a callback backend whose buffer only fills while
//! the stream is held. An OTA `SendMessage` is handled on the *command* arm of the same `select!`, and
//! `receive_ota_ack_within` opens its **own** capture stream for the ACK window. So for the whole
//! transmit + ACK window there were two concurrent input streams on one device:
//!
//! * An exclusive ALSA `hw:` device refuses the second open outright — the ACK becomes unreceivable,
//!   and the failure surfaces as a silent peer rather than as a device conflict.
//! * The daemon's own stream is not read by anyone for that whole window (up to ~9 s per attempt,
//!   times the retry budget). `CpalInputStream`'s buffer is an unbounded `VecDeque` fed from the
//!   audio callback, so the next receive tick is handed one multi-second blob of audio captured
//!   *while this station was transmitting* — never decodable, and expensive to scan.
//!
//! A `LoopbackBackend` hides both halves: its streams are clones of a shared buffer, so a second open
//! costs nothing and nothing accumulates. That is why this was filed as cpal-only and parked. It does
//! not need real hardware to test, though — it needs a backend that *reports* what a real one does.
//! `CountingBackend` below is that: it counts concurrent input streams and records the high-water
//! mark, which is the property the fix is about.
//!
//! Run: `cargo test -p openpulse-daemon --no-default-features --test ota_ack_capture_stream`

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use openpulse_config::OpenpulseConfig;
use openpulse_core::audio::{
    AudioBackend, AudioConfig, AudioInputStream, AudioOutputStream, DeviceInfo,
};
use openpulse_core::error::AudioError;
use openpulse_daemon::protocol::ControlCommand;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

// ── A backend that reports how many capture streams are open at once ──────────

#[derive(Default)]
struct Counters {
    /// Capture streams currently open.
    open_now: AtomicUsize,
    /// High-water mark of `open_now` — the number this test is about.
    open_peak: AtomicUsize,
    /// Total capture streams ever opened, so a test can tell "never opened" from "opened once".
    opened_total: AtomicUsize,
}

#[derive(Clone)]
struct CountingBackend {
    counters: Arc<Counters>,
}

impl CountingBackend {
    fn new() -> Self {
        Self {
            counters: Arc::new(Counters::default()),
        }
    }
}

struct CountingInput {
    counters: Arc<Counters>,
}

impl AudioInputStream for CountingInput {
    fn read(&mut self) -> Result<Vec<f32>, AudioError> {
        // A quiet band: no carrier, so nothing ever decodes and the ACK window runs to its deadline.
        // Sleep so a tight reader cannot spin the CPU, matching a real blocking capture.
        std::thread::sleep(Duration::from_millis(10));
        Ok(vec![0.0f32; 80])
    }

    fn close(self: Box<Self>) {}
}

impl Drop for CountingInput {
    fn drop(&mut self) {
        self.counters.open_now.fetch_sub(1, Ordering::SeqCst);
    }
}

struct SinkOutput;

impl AudioOutputStream for SinkOutput {
    fn write(&mut self, _samples: &[f32]) -> Result<(), AudioError> {
        Ok(())
    }
    fn flush(&mut self) -> Result<(), AudioError> {
        Ok(())
    }
    fn close(self: Box<Self>) {}
}

impl AudioBackend for CountingBackend {
    fn name(&self) -> &str {
        "counting"
    }

    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AudioError> {
        Ok(Vec::new())
    }

    fn open_input(
        &self,
        _device: Option<&str>,
        _config: &AudioConfig,
    ) -> Result<Box<dyn AudioInputStream>, AudioError> {
        let now = self.counters.open_now.fetch_add(1, Ordering::SeqCst) + 1;
        self.counters.opened_total.fetch_add(1, Ordering::SeqCst);
        self.counters.open_peak.fetch_max(now, Ordering::SeqCst);
        Ok(Box::new(CountingInput {
            counters: Arc::clone(&self.counters),
        }))
    }

    fn open_output(
        &self,
        _device: Option<&str>,
        _config: &AudioConfig,
    ) -> Result<Box<dyn AudioOutputStream>, AudioError> {
        Ok(Box::new(SinkOutput))
    }
}

// ── The test ──────────────────────────────────────────────────────────────────

fn cfg(tcp_port: u16, ws_port: u16) -> OpenpulseConfig {
    let mut c = OpenpulseConfig::default();
    c.station.callsign = "TESTER".into();
    // A slow rung would spend the whole test in `transmit`; this one keys and turns around promptly.
    c.modem.mode = "BPSK250".into();
    c.daemon.tcp_port = tcp_port;
    c.daemon.websocket_port = ws_port;
    c
}

/// Run a real daemon on its own thread and runtime.
///
/// `server::run`'s future is `!Send` (the engine holds an `mpsc::Receiver`), so it cannot be
/// `tokio::spawn`ed onto the test's multi-thread runtime — the same reason `twin.rs` does this.
/// The thread is detached: each test asserts on the counters and then lets the process end.
fn spawn_daemon(cfg: OpenpulseConfig, backend: CountingBackend) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("daemon runtime");
        rt.block_on(async move {
            let _ = openpulse_daemon::server::run(cfg, Box::new(backend)).await;
        });
    });
}

async fn send(w: &mut tokio::net::tcp::OwnedWriteHalf, cmd: &ControlCommand) {
    let line = serde_json::to_string(cmd).unwrap() + "\n";
    w.write_all(line.as_bytes()).await.unwrap();
}

/// THE GATE: an OTA send must not add a second concurrent capture stream.
///
/// Before the fix the peak was 2 — the receive tick's persistent stream plus the ACK window's own.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_ota_send_never_opens_a_second_concurrent_capture_stream() {
    let backend = CountingBackend::new();
    let counters = Arc::clone(&backend.counters);

    spawn_daemon(cfg(19140, 19141), backend);

    // Let the control server bind and the receive tick open its persistent capture stream.
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(
        counters.opened_total.load(Ordering::SeqCst) > 0,
        "the receive tick never opened a capture stream — the test would pass vacuously, since a \
         daemon that never captures also never opens two streams"
    );

    let stream = TcpStream::connect("127.0.0.1:19140").await.unwrap();
    let (_r, mut w) = stream.into_split();

    // An OTA session is what routes SendMessage through `ota_send_with_ptt` (transmit + ACK-wait).
    send(
        &mut w,
        &ControlCommand::StartOtaSession {
            profile: "hpx500".into(),
        },
    )
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    send(
        &mut w,
        &ControlCommand::SendMessage {
            to: "PEER".into(),
            subject: "x".into(),
            body: "ota ack capture stream".into(),
        },
    )
    .await;

    // Sample while the send is in flight. The peer is silent, so the ACK window runs its full
    // deadline (4 s for a profile without the MFSK16 sub-floor rung) — this lands inside it.
    tokio::time::sleep(Duration::from_millis(2500)).await;

    let peak = counters.open_peak.load(Ordering::SeqCst);
    assert_eq!(
        peak, 1,
        "the daemon held {peak} capture streams open at once during an OTA send; the ACK window \
         opens its own stream, so the receive tick's persistent stream must be dropped first — on \
         an exclusive ALSA device the second open fails and the ACK is never heard"
    );
}

/// The capture stream is dropped after a keyed transmit, whatever queued it.
///
/// Keyed on the transmit counter rather than the command variant, so this holds for a file-transfer
/// burst or a CONREQ as well as the OTA path — the OTA path is the one that got noticed, and its
/// siblings key PTT through the same arm.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_capture_stream_is_reopened_after_a_keyed_transmit() {
    let backend = CountingBackend::new();
    let counters = Arc::clone(&backend.counters);

    spawn_daemon(cfg(19142, 19143), backend);

    tokio::time::sleep(Duration::from_millis(400)).await;
    let before = counters.opened_total.load(Ordering::SeqCst);
    assert!(before > 0, "the receive tick never opened a capture stream");

    let stream = TcpStream::connect("127.0.0.1:19142").await.unwrap();
    let (_r, mut w) = stream.into_split();

    // No OTA session: this goes through `apply_command_to_engine`, which transmits without any
    // ACK-wait. Nothing reads the persistent stream while it transmits, so it must be dropped.
    send(
        &mut w,
        &ControlCommand::SendMessage {
            to: "PEER".into(),
            subject: "x".into(),
            body: "plain keyed transmit".into(),
        },
    )
    .await;
    tokio::time::sleep(Duration::from_millis(1500)).await;

    let after = counters.opened_total.load(Ordering::SeqCst);
    assert!(
        after > before,
        "the capture stream was never reopened after a keyed transmit ({before} opens before, \
         {after} after) — the stream held across the transmit accumulates during-transmit audio \
         that the next receive tick then has to scan"
    );
    assert_eq!(
        counters.open_peak.load(Ordering::SeqCst),
        1,
        "a plain keyed transmit must not overlap two capture streams either"
    );
}
