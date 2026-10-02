//! A transmission whose flush times out still counts as transmitted (#1334).
//!
//! `CpalOutputStream::flush` returns an error precisely when the queued samples have not finished
//! draining — the soundcard is still playing them. `record_tx_frame` used to run only after a
//! successful flush, so such a frame reached the air without bumping `frames_transmitted`, which
//! three consumers key off: the daemon's and ARDOP's station-ID timers and the daemon's post-transmit
//! capture drop. It was also missing from the §97 TX log. `LoopbackBackend::flush` always succeeds,
//! so this needs its own stream: `write` succeeds, `flush` fails. The control fails `write` instead,
//! where nothing was emitted and nothing may be recorded.

use openpulse_core::audio::{
    AudioBackend, AudioConfig, AudioInputStream, AudioIqOutputStream, AudioOutputStream, DeviceInfo,
};
use openpulse_core::error::AudioError;
use openpulse_modem::ModemEngine;

#[derive(Clone, Copy)]
enum Fault {
    /// `write` succeeds, `flush` fails — the drain timeout: audio emitted.
    Flush,
    /// `write` fails — nothing emitted.
    Write,
}

struct FaultyBackend(Fault);

struct FaultyOut(Fault);

impl FaultyOut {
    fn write_result(&self) -> Result<(), AudioError> {
        match self.0 {
            Fault::Write => Err(AudioError::Stream("write refused".into())),
            Fault::Flush => Ok(()),
        }
    }
    fn flush_result(&self) -> Result<(), AudioError> {
        match self.0 {
            Fault::Flush => Err(AudioError::Stream(
                "flush timeout: queue not drained".into(),
            )),
            Fault::Write => Ok(()),
        }
    }
}

impl AudioOutputStream for FaultyOut {
    fn write(&mut self, _samples: &[f32]) -> Result<(), AudioError> {
        self.write_result()
    }
    fn flush(&mut self) -> Result<(), AudioError> {
        self.flush_result()
    }
    fn close(self: Box<Self>) {}
}

impl AudioIqOutputStream for FaultyOut {
    fn write_iq(&mut self, _i: &[f32], _q: &[f32]) -> Result<(), AudioError> {
        self.write_result()
    }
    fn flush(&mut self) -> Result<(), AudioError> {
        self.flush_result()
    }
    fn close(self: Box<Self>) {}
}

impl AudioBackend for FaultyBackend {
    fn name(&self) -> &str {
        "faulty"
    }
    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AudioError> {
        Ok(Vec::new())
    }
    fn open_input(
        &self,
        _device: Option<&str>,
        _config: &AudioConfig,
    ) -> Result<Box<dyn AudioInputStream>, AudioError> {
        Err(AudioError::Stream("no input".into()))
    }
    fn open_output(
        &self,
        _device: Option<&str>,
        _config: &AudioConfig,
    ) -> Result<Box<dyn AudioOutputStream>, AudioError> {
        Ok(Box::new(FaultyOut(self.0)))
    }
    fn open_iq_output(
        &self,
        _device: Option<&str>,
        _config: &AudioConfig,
    ) -> Option<Result<Box<dyn AudioIqOutputStream>, AudioError>> {
        Some(Ok(Box::new(FaultyOut(self.0))))
    }
}

fn engine(fault: Fault) -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(FaultyBackend(fault)));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .expect("register BPSK");
    e
}

#[test]
fn a_flush_timeout_still_counts_the_frame() {
    let mut e = engine(Fault::Flush);
    let before = e.frames_transmitted();
    let r = e.transmit(b"on the air", "BPSK250", None);
    assert!(r.is_err(), "the flush error still reaches the caller");
    assert_eq!(
        e.frames_transmitted(),
        before + 1,
        "the samples were written and are playing — the frame was transmitted"
    );
}

#[test]
fn a_flush_timeout_on_the_iq_seam_still_counts_the_frame() {
    let mut e = engine(Fault::Flush);
    let before = e.frames_transmitted();
    let r = e.transmit_iq(b"on the air", "BPSK250", None);
    assert!(r.is_err(), "the flush error still reaches the caller");
    assert_eq!(e.frames_transmitted(), before + 1);
}

/// Control: a failed write emitted nothing, so nothing is recorded — on both seams.
#[test]
fn a_failed_write_is_not_counted() {
    let mut e = engine(Fault::Write);
    let before = e.frames_transmitted();
    assert!(e.transmit(b"never left", "BPSK250", None).is_err());
    assert!(e.transmit_iq(b"never left", "BPSK250", None).is_err());
    assert_eq!(e.frames_transmitted(), before, "nothing was emitted");
}
