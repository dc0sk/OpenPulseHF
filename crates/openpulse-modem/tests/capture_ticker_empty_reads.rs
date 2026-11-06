//! An empty capture read in the middle of a frame must not end the burst on the `CaptureTicker` path,
//! which the ARDOP TNC and KISS bridges receive through (task 16; `CaptureGap`).
//!
//! The backend delivers a frame in 400-sample reads with an EMPTY read after each, as the cpal stream
//! does when the receive loop polls between device fragments, then quiet reads. The ticker is ticked
//! every 20 ms, so the empty reads fall well inside the gap.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use openpulse_core::audio::{
    AudioBackend, AudioConfig, AudioInputStream, AudioOutputStream, DeviceInfo,
};
use openpulse_core::error::AudioError;
use openpulse_core::fec::FecMode;
use openpulse_modem::capture_ticker::CaptureTicker;
use openpulse_modem::ModemEngine;

const MODE: &str = "BPSK250";
const CHUNK: usize = 400;

#[derive(Clone)]
struct ChoppyBackend {
    frame: Arc<Mutex<Vec<f32>>>,
}

struct ChoppyStream {
    frame: Vec<f32>,
    pos: usize,
    empty_next: bool,
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
            frame: self.frame.lock().unwrap().clone(),
            pos: 0,
            empty_next: false,
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
    let mut e = ModemEngine::new(Box::new(lb.clone_shared()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .unwrap();
    e.transmit(b"ticker reads", MODE, None).expect("transmit");
    let mut samples = lb.drain_samples();
    let mut out = vec![0.0f32; 1600];
    out.append(&mut samples);
    out
}

#[test]
fn a_frame_read_with_empty_reads_between_its_chunks_comes_out_as_one_burst() {
    let frame = one_frame();
    let backend = ChoppyBackend {
        frame: Arc::new(Mutex::new(frame.clone())),
    };
    let mut engine = ModemEngine::new(Box::new(backend));
    engine
        .register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .unwrap();
    let mut ticker = CaptureTicker::new(None);
    let reads = 2 * frame.len().div_ceil(CHUNK) + 10;
    let mut bursts = Vec::new();
    for _ in 0..reads {
        if let Some(b) = ticker.tick(&mut engine, MODE).burst {
            bursts.push(b);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let first = bursts.first().expect("the frame must flush as a burst");
    assert!(
        first.samples.len() >= frame.len() - 1600,
        "the frame was cut by an empty read: first burst {} samples of a {}-sample frame ({} bursts)",
        first.samples.len(),
        frame.len(),
        bursts.len()
    );
    let payload = engine
        .decode_burst_with_fec(MODE, FecMode::None, first)
        .expect("the burst must decode");
    assert_eq!(payload, b"ticker reads");
}
