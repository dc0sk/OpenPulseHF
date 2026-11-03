//! `ModemEngine::transmit_burst` sends several frames as ONE output stream, back to back, and still
//! logs each under the sequence it was sent with (on-air task 20: two streams in one PTT hold left an
//! audio gap that the receiver took as the frame's end).

use openpulse_audio::LoopbackBackend;
use openpulse_core::audio::{
    AudioBackend, AudioConfig, AudioInputStream, AudioOutputStream, DeviceInfo,
};
use openpulse_core::error::{AudioError, ModemError};
use openpulse_core::fec::FecMode;
use openpulse_modem::ModemEngine;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn engine(backend: Box<dyn AudioBackend>) -> ModemEngine {
    let mut e = ModemEngine::new(backend);
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
        .unwrap();
    e
}

/// The loopback, counting the output streams opened.
struct Counted {
    inner: LoopbackBackend,
    outputs: Arc<AtomicUsize>,
}

impl AudioBackend for Counted {
    fn name(&self) -> &str {
        "counted"
    }
    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AudioError> {
        self.inner.list_devices()
    }
    fn open_input(
        &self,
        d: Option<&str>,
        c: &AudioConfig,
    ) -> Result<Box<dyn AudioInputStream>, AudioError> {
        self.inner.open_input(d, c)
    }
    fn open_output(
        &self,
        d: Option<&str>,
        c: &AudioConfig,
    ) -> Result<Box<dyn AudioOutputStream>, AudioError> {
        self.outputs.fetch_add(1, Ordering::SeqCst);
        self.inner.open_output(d, c)
    }
}

#[test]
fn a_burst_is_one_stream_holding_each_frame_back_to_back() {
    // The same two frames, sent separately, are the reference audio.
    let ref_bk = LoopbackBackend::new();
    let mut reference = engine(Box::new(ref_bk.clone_shared()));
    reference
        .transmit_with_fec_mode(b"the data frame", "QPSK250-D", FecMode::Rs, None)
        .unwrap();
    reference.transmit(b"DE ONESTR", "BPSK250", None).unwrap();
    let expected = ref_bk.drain_samples();

    let bk = LoopbackBackend::new();
    let outputs = Arc::new(AtomicUsize::new(0));
    let mut e = engine(Box::new(Counted {
        inner: bk.clone_shared(),
        outputs: Arc::clone(&outputs),
    }));
    e.transmit_burst(None, |e| {
        e.transmit_with_fec_mode(b"the data frame", "QPSK250-D", FecMode::Rs, None)?;
        e.transmit(b"DE ONESTR", "BPSK250", None)
    })
    .unwrap();
    assert_eq!(
        outputs.load(Ordering::SeqCst),
        1,
        "two frames in a burst must open ONE output stream"
    );
    assert_eq!(
        bk.drain_samples(),
        expected,
        "the burst's audio must be the two frames back to back, nothing between them"
    );
    assert_eq!(e.frames_transmitted(), 2);
    let log = e.tx_session_log();
    let first = log
        .get_by_sequence(0)
        .expect("the data frame logged as seq 0");
    let second = log.get_by_sequence(1).expect("the ID logged as seq 1");
    assert_eq!(first.mode, "QPSK250-D");
    assert_eq!(second.mode, "BPSK250");

    // Outside a burst, every frame opens its own stream again.
    e.transmit(b"DE ONESTR", "BPSK250", None).unwrap();
    assert_eq!(outputs.load(Ordering::SeqCst), 2);
}

#[test]
fn a_frame_that_fails_after_the_first_still_lets_the_first_go_out() {
    let bk = LoopbackBackend::new();
    let mut e = engine(Box::new(bk.clone_shared()));
    let r = e.transmit_burst(None, |e| {
        e.transmit(b"goes out", "BPSK250", None)?;
        e.transmit(b"fails", "NO-SUCH-MODE", None)
    });
    assert!(matches!(r, Err(ModemError::PluginNotFound(_))), "{r:?}");
    assert!(
        !bk.drain_samples().is_empty(),
        "the frame before the failure must still be written"
    );
    assert_eq!(e.frames_transmitted(), 1);
}
