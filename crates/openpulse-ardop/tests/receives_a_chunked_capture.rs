//! THE #1310 PR1c GATE: the ARDOP TNC accumulates a frame ACROSS READS, and holds no stream while
//! adaptive ARQ is active.
//!
//! **What was wrong.** The worker called `engine.receive`/`receive_with_fec` in a free-running poll
//! loop. `receive` opens an input stream, reads ONCE and drops it, so on a callback backend every
//! call saw one poll interval against a frame lasting seconds — this TNC could not receive on real
//! audio at all. `LoopbackBackend::read` drains its whole buffer, so the buffer WAS the frame and
//! the defect was invisible to the suite.
//!
//! **What this proves, narrowly.** Accumulation across reads and a flush on carrier drop, with a
//! SILENT fixture. Not that the TNC receives on hardware: cpal warm-up, a live noise floor and DCD
//! calibration against real band noise are all absent, and no in-process test reaches them.
//!
//! **The adaptive case is a deliberate non-conversion, and it is gated as such.** Two receives in
//! that loop open a stream of their own through `stage_capture_input` — the ISS ARQ ACK listen and
//! the adaptive IRS arm — so holding one across them would be #1007. `enable_adaptive_arq` defaults
//! to FALSE, so what is fixed here is the shipped default; the opt-in path keeps its old one-shot
//! behaviour, and the third test pins that it holds NO stream rather than silently acquiring one.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use openpulse_ardop::{spawn_worker, ModemBridge};
use openpulse_core::audio::{
    AudioBackend, AudioConfig, AudioInputStream, AudioOutputStream, DeviceInfo,
};
use openpulse_core::error::AudioError;
use openpulse_core::handshake::InMemoryTrustStore;
use openpulse_modem::ModemEngine;
use openpulse_radio::{PttController, PttError};

const MODE: &str = "BPSK250";
const PAYLOAD: &[u8] = b"\x00\x00\x00\x00\x00\x00\x00ardop over the air";
/// Far smaller than the frame, so it can only be recovered by accumulating across reads.
const CHUNK: usize = 1024;
/// Silence reads after the frame, so the flush comes from the CARRIER DROPPING, not an empty read.
const SILENCE_READS: usize = 6;

struct NoPtt;
impl PttController for NoPtt {
    fn assert_ptt(&mut self) -> Result<(), PttError> {
        Ok(())
    }
    fn release_ptt(&mut self) -> Result<(), PttError> {
        Ok(())
    }
    fn is_asserted(&self) -> bool {
        false
    }
}

fn one_frame() -> Vec<f32> {
    let lb = openpulse_audio::LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(lb.clone_shared()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .expect("register bpsk");
    e.transmit(PAYLOAD, MODE, None).expect("transmit");
    let mut samples = lb.drain_samples();
    assert!(!samples.is_empty(), "fixture frame is empty");
    let mut out = vec![0.0f32; 1600]; // lead-in, so the energy gate sees a rising edge
    out.append(&mut samples);
    out
}

#[derive(Clone)]
struct ChunkedBackend {
    pending: Arc<Mutex<Vec<f32>>>,
    frame: Vec<f32>,
    reads: Arc<AtomicUsize>,
    opens: Arc<AtomicUsize>,
    live: Arc<AtomicUsize>,
    /// High-water mark of SIMULTANEOUSLY open input streams — the #1007 observable.
    peak: Arc<AtomicUsize>,
}

struct ChunkedStream {
    pending: Arc<Mutex<Vec<f32>>>,
    frame: Vec<f32>,
    reads: Arc<AtomicUsize>,
    live: Arc<AtomicUsize>,
    silence: usize,
}

/// Decrement on DROP, not only on `close`: the engine's one-shot receives drop their stream rather
/// than closing it explicitly, so a close-only counter would never come back down and every test
/// would read a false concurrency.
impl Drop for ChunkedStream {
    fn drop(&mut self) {
        self.live.fetch_sub(1, Ordering::Relaxed);
    }
}

impl AudioInputStream for ChunkedStream {
    fn read(&mut self) -> Result<Vec<f32>, AudioError> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        let mut g = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if g.is_empty() {
            self.silence += 1;
            if self.silence >= SILENCE_READS && !self.frame.is_empty() {
                self.silence = 0;
                *g = self.frame.clone();
            }
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
        let now = self.live.fetch_add(1, Ordering::Relaxed) + 1;
        self.peak.fetch_max(now, Ordering::Relaxed);
        Ok(Box::new(ChunkedStream {
            pending: Arc::clone(&self.pending),
            frame: self.frame.clone(),
            reads: Arc::clone(&self.reads),
            live: Arc::clone(&self.live),
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

/// `adaptive` starts an adaptive session, which is what makes the worker refuse to hold a stream.
fn rig(frame: Vec<f32>, adaptive: bool) -> (Arc<ModemBridge>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let backend = ChunkedBackend {
        pending: Arc::new(Mutex::new(frame.clone())),
        frame,
        reads: Arc::new(AtomicUsize::new(0)),
        opens: Arc::new(AtomicUsize::new(0)),
        live: Arc::new(AtomicUsize::new(0)),
        peak: Arc::new(AtomicUsize::new(0)),
    };
    let (reads, peak) = (Arc::clone(&backend.reads), Arc::clone(&backend.peak));
    let mut engine = ModemEngine::new(Box::new(backend));
    engine
        .register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .expect("register bpsk");
    if adaptive {
        engine.start_adaptive_session(openpulse_core::profile::SessionProfile::hpx500());
    }
    let (bridge, tx_rx) = ModemBridge::with_ptt(
        engine,
        MODE.into(),
        false, // non-loopback: the real RX path
        InMemoryTrustStore::default(),
        None,
        Box::new(NoPtt),
    );
    *bridge.callsign.try_write().expect("callsign") = "DC0SK".into();
    spawn_worker(Arc::clone(&bridge), tx_rx);
    (bridge, reads, peak)
}

#[test]
fn a_frame_delivered_in_chunks_is_received() {
    let frame = one_frame();
    assert!(
        CHUNK < frame.len(),
        "fixture is vacuous: one {CHUNK}-sample read would hold the whole {}-sample frame",
        frame.len()
    );
    let (bridge, reads, _) = rig(frame, false);
    let mut rx = bridge.rx_data_tx.subscribe();

    let deadline = Instant::now() + Duration::from_secs(30);
    let mut got = None;
    while Instant::now() < deadline {
        if let Ok(p) = rx.try_recv() {
            got = Some(p);
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
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
    assert!(
        reads.load(Ordering::Relaxed) > 1,
        "only one read occurred: the fixture is not delivering the frame in chunks"
    );
}

/// The same frame must decode with `FECRCV` set, because `fec_rx` is a STICKY flag and the burst is
/// tried as `[Rs, None]` rather than either/or. Before this, one `FECRCV` made a station deaf to
/// every uncoded frame for the rest of the session — the peer's station ID and any relay envelope.
#[test]
fn an_uncoded_frame_still_decodes_with_fec_rx_set() {
    let frame = one_frame();
    let (bridge, reads, _) = rig(frame, false);
    bridge.fec_rx.store(true, Ordering::Relaxed);
    let mut rx = bridge.rx_data_tx.subscribe();

    let deadline = Instant::now() + Duration::from_secs(30);
    let mut got = None;
    while Instant::now() < deadline {
        if let Ok(p) = rx.try_recv() {
            got = Some(p);
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let payload = got.unwrap_or_else(|| {
        panic!(
            "an UNCODED frame was lost with FECRCV set, after {} reads — the sticky flag is still \
             either/or rather than [Rs, None]",
            reads.load(Ordering::Relaxed)
        )
    });
    assert_eq!(payload, PAYLOAD);
}

/// The deliberate non-conversion, pinned on the property that actually matters.
///
/// With an adaptive session live, the ACK listen and the adaptive IRS arm each open a capture stream
/// of their own through `stage_capture_input`. If the worker ALSO held one, two would be open at the
/// same time on one device — which is #1007, and on an exclusive device the second open simply fails.
///
/// **The observable is the high-water mark of SIMULTANEOUSLY live streams, not the open count.** An
/// open count cannot discriminate here and a first draft of this test was vacuous because of it: the
/// adaptive arm reopens per call either way, so the count climbs whether or not a ticker is held.
/// Measured — with the adaptive guard sabotaged to `if false`, the count-based version still passed.
#[test]
fn no_two_capture_streams_are_ever_open_at_once_under_adaptive_arq() {
    let (_bridge, reads, peak) = rig(Vec::new(), true); // silence-only: no decode noise
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline && reads.load(Ordering::Relaxed) < 8 {
        std::thread::sleep(Duration::from_millis(20));
    }
    let (r, p) = (reads.load(Ordering::Relaxed), peak.load(Ordering::Relaxed));
    assert!(r >= 8, "the worker never ticked: {r} reads");
    assert_eq!(
        p, 1,
        "{p} capture streams were open at once across {r} reads — the worker is holding a stream \
         while adaptive ARQ is active, concurrent with the ACK listen's own capture (#1007)"
    );
}
