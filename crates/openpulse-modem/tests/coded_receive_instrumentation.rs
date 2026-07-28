//! The coded (FEC) scanning receive must report what it did, per attempt.
//!
//! **The gap this pins.** `receive_from_samples_with_fec` carried **zero** log statements while its
//! uncoded twin `receive_from_samples` carried three. On air that difference is the whole ballgame:
//! when `BPSK250|rs` and `BPSK250|soft_concatenated` failed to decode over a real 2 m link
//! (issue #1021) while `BPSK250|none` passed in the same session, the uncoded run produced 920
//! "demodulated N bytes" and 115 per-position failure lines to reason from, and the coded runs
//! produced **nothing at all** — no attempt count, no demodulated length, no codec error. The
//! failure could not be diagnosed from a real capture, only guessed at.
//!
//! Diagnosing a radio problem from a log you cannot re-capture at will is the normal case on air:
//! the run costs a keyed transmission and an agreed time window with a second operator. So the
//! instrumentation is load-bearing, and it is asserted here rather than left to survive by luck —
//! delete either `debug!` in `receive_from_samples_with_fec*` and these tests fail.
//!
//! These assertions deliberately check BOTH outcomes: a decode that fails must say so (that is the
//! on-air case), and a decode that succeeds must report its payload length (so a future "log only
//! on error" refactor cannot silently blind the success path).

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bpsk_plugin::BpskPlugin;
use openpulse_core::fec::FecMode;
use openpulse_modem::channel_sim::ChannelSimHarness;
use tracing::Level;
use tracing_subscriber::fmt::MakeWriter;

/// Shared in-memory sink so the test can read back what was logged.
#[derive(Clone, Default)]
struct LogBuf(Arc<Mutex<Vec<u8>>>);

impl LogBuf {
    fn contents(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().expect("log buffer poisoned")).to_string()
    }
}

impl Write for LogBuf {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .expect("log buffer poisoned")
            .extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for LogBuf {
    type Writer = LogBuf;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Run `f` with a DEBUG-level subscriber installed and return everything it logged.
fn capture_logs<F: FnOnce()>(f: F) -> String {
    let buf = LogBuf::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .with_writer(buf.clone())
        .without_time()
        .finish();
    tracing::subscriber::with_default(subscriber, f);
    buf.contents()
}

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for eng in [&mut h.tx_engine, &mut h.rx_engine] {
        eng.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    }
    h
}

/// A coded receive over a capture longer than the frame — the on-air shape.
fn coded_round_trip(fec: FecMode, payload: &[u8]) -> Result<Vec<u8>, String> {
    let mut h = harness();
    h.tx_engine
        .transmit_with_fec_mode(payload, "BPSK250", fec, None)
        .map_err(|e| format!("transmit: {e}"))?;
    let frame_samples = h.route_embedded(4000, 4000);
    assert!(
        frame_samples > 0,
        "nothing was transmitted — the test would prove nothing"
    );
    h.rx_engine
        .receive_with_fec_mode_timeout("BPSK250", fec, None, Duration::from_millis(6000))
        .map_err(|e| format!("{e}"))
}

#[test]
fn coded_receive_logs_the_demodulated_length_and_the_attempt_outcome() {
    let payload = b"coded receive instrumentation probe".to_vec();
    let logs = capture_logs(|| {
        let _ = coded_round_trip(FecMode::Rs, &payload);
    });

    assert!(
        logs.contains("fec demod:"),
        "the coded receive must report what the demodulator produced (wire bytes / LLR count); \
         without it an on-air failure cannot be told apart from a demod that yielded nothing.\n\
         captured logs:\n{logs}"
    );
    assert!(
        logs.contains("fec attempt"),
        "the coded receive must report each attempt's outcome; this is the line whose absence \
         made issue #1021 undiagnosable from a real capture.\n\
         captured logs:\n{logs}"
    );
    // The mode and FEC must be identifiable: a bare "failed" line cannot be attributed to a rung
    // when a matrix run interleaves several.
    assert!(
        logs.contains("BPSK250") && logs.contains("Rs"),
        "attempt logging must identify the mode and FEC it describes.\ncaptured logs:\n{logs}"
    );
}

#[test]
fn a_failing_coded_receive_reports_the_failure_reason() {
    // Transmit under one codec and receive under another: the demodulator still produces bytes,
    // but the codec cannot decode them, so every attempt fails. That is the on-air shape of
    // issue #1021 (a receiver that decodes nothing) with a deterministic cause, and it must leave
    // evidence rather than an empty log.
    let payload = b"codec mismatch probe".to_vec();
    let logs = capture_logs(|| {
        let mut h = harness();
        h.tx_engine
            .transmit_with_fec_mode(&payload, "BPSK250", FecMode::Rs, None)
            .expect("transmit");
        let frame_samples = h.route_embedded(4000, 4000);
        assert!(
            frame_samples > 0,
            "nothing transmitted — test would prove nothing"
        );
        let got = h.rx_engine.receive_with_fec_mode_timeout(
            "BPSK250",
            FecMode::RsInterleaved,
            None,
            Duration::from_millis(2500),
        );
        assert!(
            got.is_err(),
            "receiving Rs data as RsInterleaved must not decode — otherwise this test is not \
             exercising the failure path it claims to"
        );
    });

    assert!(
        logs.contains("fec attempt FAILED") || logs.contains("fec demod:"),
        "a coded receive that decodes nothing must still leave evidence of what it tried; \
         silence here is the #1021 defect.\ncaptured logs:\n{logs}"
    );
}

#[test]
fn a_successful_coded_receive_reports_its_payload_length() {
    let payload = b"successful coded decode".to_vec();
    let mut logs = String::new();
    let mut decoded = None;
    // The channel is clean, but acquisition is not guaranteed on any single run; retry a few
    // times so the assertion is about LOGGING, never about acquisition luck.
    for _ in 0..6 {
        let mut got = None;
        logs = capture_logs(|| {
            got = coded_round_trip(FecMode::Rs, &payload).ok();
        });
        if got.is_some() {
            decoded = got;
            break;
        }
    }

    let decoded =
        decoded.expect("a clean-channel coded round trip should decode within 6 attempts");
    assert_eq!(
        decoded, payload,
        "round trip must return the payload intact"
    );
    assert!(
        logs.contains("fec attempt OK"),
        "a successful coded decode must be logged too — otherwise a 'log only on error' \
         refactor blinds the success path without failing any test.\ncaptured logs:\n{logs}"
    );
}
