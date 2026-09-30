//! REQ-CMP-05 through the daemon's real receive tick: a frame that carries the `OPZ1` pack magic but
//! does not decompress is a frame-integrity error — dropped, not routed as raw compressed bytes.
//!
//! **The defect this pins.** `server.rs` did `unpack(&bytes).unwrap_or(bytes)`, and `unpack` answered
//! `None` both for "not a packed frame" and for "a packed frame that failed", so the second was
//! delivered as the raw compressed bytes. That included a frame whose zstd dictionary did not match
//! — i.e. every frame from a peer with a retrained dictionary.
//!
//! **What is observed, and why it cannot pass vacuously.** The daemon exposes no per-frame delivery
//! event, but the rx tick feeds every delivered payload into the live compressibility metric, so
//! `Metrics.compress_ratio` stays `None` until a payload is delivered. The refusal case first waits
//! for `FrameReceived` (the modem DID decode the frame) and only then requires the metric to stay
//! `None` across several snapshots; each control delivers a frame (one packed, one not) that must
//! turn it `Some`.

use std::time::Duration;

use openpulse_config::OpenpulseConfig;
use openpulse_core::audio::{
    AudioBackend, AudioConfig, AudioInputStream, AudioOutputStream, DeviceInfo,
};
use openpulse_core::compression::{pack, PACK_MAGIC, ZSTD_DICT_ID};
use openpulse_core::error::AudioError;
use openpulse_daemon::protocol::ControlEvent;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpStream;

const MODE: &str = "BPSK250";

/// Replays one prepared burst, then silence, then the burst again — recurring, so a control client
/// that connects after the first flush still sees one (see `monitor_during_ota.rs` for why).
#[derive(Clone)]
struct ReplayBackend {
    frame: Vec<f32>,
}

struct ReplayStream {
    pending: Vec<f32>,
    frame: Vec<f32>,
    silence_reads: usize,
}

const SILENCE_READS_BETWEEN_BURSTS: usize = 4;

impl AudioInputStream for ReplayStream {
    fn read(&mut self) -> Result<Vec<f32>, AudioError> {
        if self.pending.is_empty() {
            self.silence_reads += 1;
            if self.silence_reads >= SILENCE_READS_BETWEEN_BURSTS {
                self.silence_reads = 0;
                self.pending = self.frame.clone();
            }
            Ok(vec![0.0; 800])
        } else {
            let take = self.pending.len().min(4096);
            Ok(self.pending.drain(..take).collect())
        }
    }
    fn close(self: Box<Self>) {}
}

struct NullOut;
impl AudioOutputStream for NullOut {
    fn write(&mut self, _samples: &[f32]) -> Result<(), AudioError> {
        Ok(())
    }
    fn flush(&mut self) -> Result<(), AudioError> {
        Ok(())
    }
    fn close(self: Box<Self>) {}
}

impl AudioBackend for ReplayBackend {
    fn name(&self) -> &str {
        "Replay"
    }
    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AudioError> {
        Ok(vec![])
    }
    fn open_input(
        &self,
        _d: Option<&str>,
        _c: &AudioConfig,
    ) -> Result<Box<dyn AudioInputStream>, AudioError> {
        Ok(Box::new(ReplayStream {
            pending: self.frame.clone(),
            frame: self.frame.clone(),
            silence_reads: 0,
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

/// Modulate `payload` as one uncoded `MODE` frame with a throwaway engine, lead-in silence included.
fn frame_audio(payload: &[u8]) -> Vec<f32> {
    let lb = openpulse_audio::LoopbackBackend::new();
    let mut e = openpulse_modem::ModemEngine::new(Box::new(lb.clone_shared()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .unwrap();
    e.transmit(payload, MODE, None).expect("transmit");
    let mut samples = lb.drain_samples();
    assert!(!samples.is_empty(), "fixture frame is empty");
    let mut out = vec![0.0f32; 1600];
    out.append(&mut samples);
    out
}

fn spawn_daemon(tcp_port: u16, ws_port: u16, payload: &[u8]) {
    let mut c = OpenpulseConfig::default();
    c.station.callsign = "TESTER".into();
    c.modem.mode = MODE.into();
    c.daemon.tcp_port = tcp_port;
    c.daemon.websocket_port = ws_port;
    let backend = ReplayBackend {
        frame: frame_audio(payload),
    };
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("daemon runtime");
        rt.block_on(async move {
            let _ = openpulse_daemon::server::run(c, Box::new(backend)).await;
        });
    });
}

/// What the control stream showed: whether a frame was decoded, and the `compress_ratio` of every
/// `Metrics` snapshot that arrived AFTER the first decode.
struct Observed {
    decoded: bool,
    ratios_after_decode: Vec<Option<f32>>,
}

/// Watch the control stream until `snapshots` metrics have arrived after the first decode, a
/// delivered payload shows up (`Some` ratio), or `deadline` passes.
async fn observe(tcp_port: u16, snapshots: usize, deadline: Duration) -> Observed {
    tokio::time::sleep(Duration::from_millis(400)).await;
    let stream = TcpStream::connect(("127.0.0.1", tcp_port))
        .await
        .expect("control port");
    let (r, _w) = stream.into_split();
    let mut reader = BufReader::new(r);
    let mut seen = Observed {
        decoded: false,
        ratios_after_decode: Vec::new(),
    };
    let _ = tokio::time::timeout(deadline, async {
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                return;
            }
            match serde_json::from_str::<ControlEvent>(line.trim()) {
                Ok(ControlEvent::EngineEvent {
                    event: openpulse_modem::EngineEvent::FrameReceived { bytes, .. },
                }) if bytes > 0 => seen.decoded = true,
                Ok(ControlEvent::Metrics { compress_ratio, .. }) if seen.decoded => {
                    seen.ratios_after_decode.push(compress_ratio);
                    if compress_ratio.is_some() || seen.ratios_after_decode.len() >= snapshots {
                        return;
                    }
                }
                _ => {}
            }
        }
    })
    .await;
    seen
}

// VERIFIES: REQ-CMP-05
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_packed_frame_that_fails_to_decompress_is_not_delivered() {
    // A zstd frame naming a dictionary this build does not have.
    let mut payload = PACK_MAGIC.to_vec();
    payload.push(3);
    payload.extend_from_slice(&(ZSTD_DICT_ID ^ 1).to_le_bytes());
    payload.extend_from_slice(b"\x00\x00\x00\x20not a zstd frame for this dictionary");
    spawn_daemon(19200, 19201, &payload);

    let seen = observe(19200, 4, Duration::from_secs(25)).await;
    assert!(
        seen.decoded,
        "the modem never decoded the fixture frame, so this run proves nothing about the unpack"
    );
    assert!(
        seen.ratios_after_decode.iter().all(Option::is_none),
        "a packed frame that failed to decompress was delivered as a payload (REQ-CMP-05): {:?}",
        seen.ratios_after_decode
    );
    assert!(
        seen.ratios_after_decode.len() >= 4,
        "too few metrics snapshots after the decode to judge: {:?}",
        seen.ratios_after_decode
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_good_packed_frame_is_delivered() {
    spawn_daemon(19202, 19203, &pack(&b"status ok ".repeat(12)));
    let seen = observe(19202, 20, Duration::from_secs(25)).await;
    assert!(seen.decoded, "the modem never decoded the fixture frame");
    assert!(
        seen.ratios_after_decode.iter().any(Option::is_some),
        "positive control: a well-formed packed frame was not delivered: {:?}",
        seen.ratios_after_decode
    );
}

// VERIFIES: REQ-CMP-03
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_frame_without_the_magic_is_delivered_unchanged() {
    spawn_daemon(19204, 19205, b"plain uncompressed traffic");
    let seen = observe(19204, 20, Duration::from_secs(25)).await;
    assert!(seen.decoded, "the modem never decoded the fixture frame");
    assert!(
        seen.ratios_after_decode.iter().any(Option::is_some),
        "a non-packed frame was not delivered: {:?}",
        seen.ratios_after_decode
    );
}
