//! THE #1310 PR1a GATE: the KISS TNC accumulates a frame ACROSS READS and decodes it.
//!
//! **What was wrong.** `worker_loop` called `engine.receive(&mode, None)` twice per iteration.
//! `receive` opens an input stream, reads ONCE, and drops the stream — so on a callback backend
//! every call saw a fresh buffer covering one 5 ms poll, against a frame lasting seconds. This TNC
//! could not receive on real audio at all, however long it ran.
//!
//! **Why the whole suite stayed green over it.** `LoopbackBackend::read` drains its entire buffer,
//! so in every existing test the buffer IS the frame and one read is enough. The defect is
//! structurally invisible to any fixture built on that backend, which is why this file supplies its
//! own backend and hands the frame over in pieces.
//!
//! **What this proves, stated narrowly.** That the front end accumulates across reads and decodes on
//! a carrier drop, with a SILENT fixture. It is not evidence that the TNC receives on hardware: cpal
//! stream warm-up, a live noise floor, and the DCD calibration against real band noise are all
//! absent here. Those belong to the on-air tier (#1112's shape), and no in-process test reaches them.
//!
//! **The discriminator is chunk SIZE, not chunk alignment.** #1247 needed a misaligned fixture
//! because that path also ran a whole-buffer decode per raw chunk; this path decodes nothing until
//! the accumulator flushes, so alignment cannot make it pass vacuously. What makes it fail on the old
//! code is that NO SINGLE READ HOLDS A DECODABLE FRAME — asserted below, so the fixture cannot
//! silently become one-read-is-enough if the frame geometry changes.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use openpulse_core::audio::{
    AudioBackend, AudioConfig, AudioInputStream, AudioOutputStream, DeviceInfo,
};
use openpulse_core::error::AudioError;
use openpulse_kiss::{spawn_worker, KissBridge};
use openpulse_modem::ModemEngine;

const MODE: &str = "BPSK250";
const PAYLOAD: &[u8] = b"\x00\x00\x00\x00\x00\x00\x00kiss over the air";

/// Samples handed over per `read()`. Deliberately far smaller than the frame, so the frame can only
/// be recovered by accumulating across reads.
const CHUNK: usize = 1024;

/// Silence reads after the frame, so `accumulate_capture` sees the carrier DROP and flushes. The
/// flush therefore comes from DCD energy falling below the squelch, not from an empty read.
const SILENCE_READS: usize = 6;

fn one_frame() -> Vec<f32> {
    let lb = openpulse_audio::LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(lb.clone_shared()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .expect("register bpsk");
    e.transmit(PAYLOAD, MODE, None).expect("transmit");
    let mut samples = lb.drain_samples();
    assert!(!samples.is_empty(), "fixture frame is empty");
    // Lead-in silence so the energy gate sees a rising edge rather than starting mid-carrier.
    let mut out = vec![0.0f32; 1600];
    out.append(&mut samples);
    out
}

#[derive(Clone)]
struct ChunkedBackend {
    pending: Arc<Mutex<Vec<f32>>>,
    frame: Vec<f32>,
    reads: Arc<AtomicUsize>,
    opens: Arc<AtomicUsize>,
}

struct ChunkedStream {
    pending: Arc<Mutex<Vec<f32>>>,
    frame: Vec<f32>,
    reads: Arc<AtomicUsize>,
    silence: usize,
}

impl AudioInputStream for ChunkedStream {
    fn read(&mut self) -> Result<Vec<f32>, AudioError> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        let mut g = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if g.is_empty() {
            self.silence += 1;
            if self.silence >= SILENCE_READS {
                self.silence = 0;
                *g = self.frame.clone();
            }
            // Silence, not an empty read: the flush must come from the carrier dropping.
            return Ok(vec![0.0; 800]);
        }
        let take = g.len().min(CHUNK);
        Ok(g.drain(..take).collect())
    }
    fn close(self: Box<Self>) {}
}

struct NullOut;
impl AudioOutputStream for NullOut {
    fn write(&mut self, _s: &[f32]) -> Result<(), AudioError> {
        Ok(())
    }
    fn flush(&mut self) -> Result<(), AudioError> {
        Ok(())
    }
    fn close(self: Box<Self>) {}
}

impl AudioBackend for ChunkedBackend {
    fn name(&self) -> &str {
        "Chunked"
    }
    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AudioError> {
        Ok(vec![])
    }
    fn open_input(
        &self,
        _d: Option<&str>,
        _c: &AudioConfig,
    ) -> Result<Box<dyn AudioInputStream>, AudioError> {
        self.opens.fetch_add(1, Ordering::Relaxed);
        Ok(Box::new(ChunkedStream {
            pending: Arc::clone(&self.pending),
            frame: self.frame.clone(),
            reads: Arc::clone(&self.reads),
            silence: 0,
        }))
    }
    fn open_output(
        &self,
        _d: Option<&str>,
        _c: &AudioConfig,
    ) -> Result<Box<dyn AudioOutputStream>, AudioError> {
        Ok(Box::new(NullOut))
    }
}

#[test]
fn a_frame_delivered_in_chunks_is_received() {
    let frame = one_frame();

    // The fixture's own precondition: no single read can hold the frame. Without this the test could
    // pass on the pre-fix `receive()` path and nobody would know it had gone vacuous.
    assert!(
        CHUNK < frame.len(),
        "fixture is vacuous: one {CHUNK}-sample read would hold the whole {}-sample frame",
        frame.len()
    );

    let backend = ChunkedBackend {
        pending: Arc::new(Mutex::new(frame.clone())),
        frame,
        reads: Arc::new(AtomicUsize::new(0)),
        opens: Arc::new(AtomicUsize::new(0)),
    };
    let reads = Arc::clone(&backend.reads);

    let mut engine = ModemEngine::new(Box::new(backend.clone()));
    engine
        .register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .expect("register bpsk");

    // `loopback: false` is what puts the worker on the real RX path.
    let (bridge, tx_rx) = KissBridge::new(engine, MODE.to_string(), false);
    let mut rx = bridge.rx_data_tx.subscribe();
    spawn_worker(Arc::clone(&bridge), tx_rx);

    let deadline = Instant::now() + Duration::from_secs(30);
    let mut got = None;
    while Instant::now() < deadline {
        match rx.try_recv() {
            Ok(payload) => {
                got = Some(payload);
                break;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }

    let payload = got.unwrap_or_else(|| {
        panic!(
            "no frame received within 30 s after {} reads — the worker never accumulated a burst",
            reads.load(Ordering::Relaxed)
        )
    });
    assert_eq!(
        payload, PAYLOAD,
        "the received payload is not the transmitted one"
    );

    // A tripwire that the frame really did arrive in pieces. One read per 5 ms tick over a frame
    // longer than CHUNK cannot be satisfied by a single read, so a backend that started draining
    // everything (the LoopbackBackend shape this file exists to avoid) would show up here.
    assert!(
        reads.load(Ordering::Relaxed) > 1,
        "only one read occurred: the fixture is not delivering the frame in chunks"
    );
}

/// THE OTHER HALF (#1310 item 5): holding a stream CREATES a post-transmit obligation this TNC did
/// not have before.
///
/// While the worker held no stream, a transmit was harmless to the capture path. Now that one stream
/// lives across ticks, keying with it open leaves it UNREAD for the whole emission: its buffer fills
/// with this station's own transmitted audio and the next tick hands that blob to
/// `accumulate_capture`. On an exclusive device a concurrent open fails outright. That is #1007 and
/// #1319, and `CaptureTicker::drop_stream` before keying is what discharges it.
///
/// **The observable is the OPEN COUNT**, which is attributable here because nothing else can move it:
/// this backend never returns a read error, and a read error is the only other thing that drops the
/// ticker's stream. So a second open means the deliberate drop happened.
///
/// The fixture is pure silence — no frame — so no burst, no decode, and nothing but the transmit can
/// touch the stream.
#[test]
fn a_transmit_drops_the_held_capture_stream() {
    let backend = ChunkedBackend {
        pending: Arc::new(Mutex::new(Vec::new())),
        frame: Vec::new(), // silence forever
        reads: Arc::new(AtomicUsize::new(0)),
        opens: Arc::new(AtomicUsize::new(0)),
    };
    let opens = Arc::clone(&backend.opens);
    let reads = Arc::clone(&backend.reads);

    let mut engine = ModemEngine::new(Box::new(backend.clone()));
    engine
        .register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .expect("register bpsk");

    let (bridge, tx_rx) = KissBridge::new(engine, MODE.to_string(), false);
    spawn_worker(Arc::clone(&bridge), tx_rx);

    // Wait for the ticker to open and HOLD one stream across several ticks. The held-ness is the
    // precondition: if the stream were reopened per tick, `opens` would climb with `reads` and this
    // test could not tell a deliberate drop from the old behaviour.
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && reads.load(Ordering::Relaxed) < 5 {
        std::thread::sleep(Duration::from_millis(20));
    }
    let opens_before = opens.load(Ordering::Relaxed);
    let reads_before = reads.load(Ordering::Relaxed);
    assert!(
        reads_before >= 5,
        "the worker never ticked: {reads_before} reads"
    );
    assert_eq!(
        opens_before, 1,
        "expected ONE held stream across {reads_before} reads, saw {opens_before} opens — \
         the stream is not being held, so this test cannot detect a deliberate drop"
    );

    // A valid AX.25 UI frame: KISS refuses to key for a frame with no decodable source callsign
    // (§97.119), so an arbitrary byte string would be dropped before the transmit and this test
    // would pass vacuously on any build.
    let frame = openpulse_kiss::ax25::Ax25UiFrame {
        dest: openpulse_kiss::ax25::Ax25Addr::parse("APRS").expect("dest"),
        src: openpulse_kiss::ax25::Ax25Addr::parse("W1AW-9").expect("src"),
        info: b"drop the stream".to_vec(),
    }
    .encode()
    .expect("encode AX.25");
    bridge.tx_data_tx.send(frame).expect("queue TX");

    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline && opens.load(Ordering::Relaxed) < 2 {
        std::thread::sleep(Duration::from_millis(20));
    }

    assert_eq!(
        opens.load(Ordering::Relaxed),
        2,
        "the capture stream was not reopened after the transmit: it was held OPEN across the \
         emission, so it accumulated this station's own audio (#1007/#1319)"
    );
}
