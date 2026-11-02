//! An empty capture read in the middle of a frame must not end the burst.
//!
//! A real capture stream returns an empty read whenever the device has delivered nothing since the last
//! receive tick (`CpalInputStream::read` returns after 10 ms with whatever arrived). The engine counts a
//! read as carrier only when it holds samples, so each such read ended the burst as if the carrier had
//! gone. On air (task 16, 2026-11-02) 228 of station A's 234 carrier-drop flushes, and 118 of station
//! B's 124, were empty reads with the signal above the squelch: frames were cut into hundreds of pieces
//! and none of them decoded. The replay harnesses never produce an empty read, so nothing offline
//! showed it.
//!
//! This drives the real `server::run` receive tick with a backend that delivers a frame in 400-sample
//! chunks with an empty read after each, the way the cpal stream does, and asserts the frame decodes
//! (seen as a `MonitorFrame` on the control port).

use std::time::Duration;

use openpulse_config::OpenpulseConfig;
use openpulse_core::audio::{
    AudioBackend, AudioConfig, AudioInputStream, AudioOutputStream, DeviceInfo,
};
use openpulse_core::error::AudioError;
use openpulse_daemon::protocol::ControlEvent;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpStream;

const MODE: &str = "BPSK250";
const CHUNK: usize = 400;
/// Quiet, non-empty reads between repeats of the frame, so the burst ends on real silence.
const SILENCE_READS: usize = 6;

#[derive(Clone)]
struct ChoppyBackend {
    frame: Vec<f32>,
}

/// Delivers the frame in `CHUNK`-sample reads with an EMPTY read after each, then quiet reads, then
/// repeats (the control client connects after the daemon starts, and a broadcast with no subscriber
/// is dropped; see `monitor_during_ota.rs`).
struct ChoppyStream {
    frame: Vec<f32>,
    pos: usize,
    empty_next: bool,
    silence: usize,
}

impl AudioInputStream for ChoppyStream {
    fn read(&mut self) -> Result<Vec<f32>, AudioError> {
        if self.pos < self.frame.len() {
            if self.empty_next {
                self.empty_next = false;
                return Ok(Vec::new());
            }
            let end = (self.pos + CHUNK).min(self.frame.len());
            let out = self.frame[self.pos..end].to_vec();
            self.pos = end;
            self.empty_next = true;
            return Ok(out);
        }
        self.silence += 1;
        if self.silence >= SILENCE_READS {
            self.silence = 0;
            self.pos = 0;
        }
        Ok(vec![0.0; 800])
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

impl AudioBackend for ChoppyBackend {
    fn name(&self) -> &str {
        "Choppy"
    }
    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AudioError> {
        Ok(vec![])
    }
    fn open_input(
        &self,
        _d: Option<&str>,
        _c: &AudioConfig,
    ) -> Result<Box<dyn AudioInputStream>, AudioError> {
        Ok(Box::new(ChoppyStream {
            frame: self.frame.clone(),
            pos: 0,
            empty_next: false,
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

fn one_frame() -> Vec<f32> {
    let lb = openpulse_audio::LoopbackBackend::new();
    let mut e = openpulse_modem::ModemEngine::new(Box::new(lb.clone_shared()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .unwrap();
    e.transmit(b"empty reads", MODE, None).expect("transmit");
    let mut samples = lb.drain_samples();
    assert!(!samples.is_empty(), "fixture frame is empty");
    let mut out = vec![0.0f32; 1600];
    out.append(&mut samples);
    out
}

fn cfg(tcp_port: u16, ws_port: u16) -> OpenpulseConfig {
    let mut c = OpenpulseConfig::default();
    c.station.callsign = "TESTER".into();
    c.modem.mode = MODE.into();
    c.daemon.tcp_port = tcp_port;
    c.daemon.websocket_port = ws_port;
    c.monitor.enabled = true;
    c.monitor.modes = vec![MODE.into()];
    c
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_frame_read_with_empty_reads_between_its_chunks_still_decodes() {
    let backend = ChoppyBackend { frame: one_frame() };
    let cfg = cfg(19170, 19171);
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
    tokio::time::sleep(Duration::from_millis(400)).await;

    let stream = TcpStream::connect("127.0.0.1:19170")
        .await
        .expect("control port");
    let (r, _w) = stream.into_split();
    let mut reader = BufReader::new(r);

    let decoded = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                return None;
            }
            if let Ok(ControlEvent::MonitorFrame { bytes, .. }) =
                serde_json::from_str::<ControlEvent>(line.trim())
            {
                return Some(bytes);
            }
        }
    })
    .await
    .unwrap_or(None);

    assert_eq!(
        decoded.as_deref(),
        Some(&b"empty reads"[..]),
        "a frame delivered with an empty read between its chunks never decoded: each empty read ended \
         the burst as if the carrier had dropped (task 16's mid-frame cuts). See \
         `empty_read_is_silence` in server.rs"
    );
}
