//! Phase E-3: the `transmit_raw_audio` seam emits pre-built (e.g. JS8 beacon) audio through the
//! OutputEmit stage — counted by the tripwire and gated by CSMA — without the HPX Frame envelope.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::error::ModemError;
use openpulse_modem::ModemEngine;

#[test]
fn transmit_raw_audio_emits_and_increments_the_tripwire() {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    assert_eq!(e.raw_audio_frames_transmitted(), 0);
    let audio = vec![0.1f32; 8000];
    e.transmit_raw_audio(&audio, "JS8-NORMAL", None).unwrap();
    e.transmit_raw_audio(&audio, "JS8-NORMAL", None).unwrap();
    assert_eq!(e.raw_audio_frames_transmitted(), 2);
}

#[test]
fn transmit_raw_audio_defers_on_a_busy_channel() {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    e.register_plugin(Box::new(BpskPlugin::new())).unwrap();

    // Make the channel busy, then enable CSMA.
    e.transmit(b"remote", "BPSK250", None).unwrap();
    let _ = e.receive("BPSK250", None).unwrap();
    assert!(e.is_channel_busy());
    e.enable_csma();

    let audio = vec![0.1f32; 8000];
    let r = e.transmit_raw_audio(&audio, "JS8-NORMAL", None);
    assert!(
        matches!(r, Err(ModemError::ChannelBusy)),
        "raw audio must defer on a busy channel"
    );
    assert_eq!(e.raw_audio_frames_transmitted(), 0);
}
