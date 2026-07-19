//! Latched stream-fault state shared between an audio driver callback and its reader/writer
//! (audit 2026-07-19, finding #19).
//!
//! **Why this exists.** A cpal stream reports device loss — USB unplug, card reset, server restart —
//! only through its error callback. That callback logged and discarded the error, and `read()`
//! returns `Ok(vec![])` whenever its buffer is empty, so after the device died the caller saw an
//! unbroken sequence of successful empty reads. An unattended station went deaf **silently**: no
//! error, no event, no recovery, and "quiet band" and "sound card gone" were indistinguishable.
//!
//! This type is deliberately in an **ungated** module, not behind `cpal-backend`. The workspace test
//! suite runs `--no-default-features`, so anything living inside the cpal module is untestable in the
//! gate that actually runs — and an untested fault path is how the original defect survived.

use std::sync::{Arc, Mutex};

use openpulse_core::error::AudioError;

/// First-error-wins latch for a fatal stream error.
///
/// Cloning shares the latch, so the driver callback and the reader observe the same state.
#[derive(Clone, Default)]
pub struct StreamFault(Arc<Mutex<Option<String>>>);

impl StreamFault {
    /// A latch with no fault recorded.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a fatal stream error. **The first error wins**: a dying device typically emits the same
    /// error every callback period, and the first one is the diagnostic that names the actual cause —
    /// later ones are consequences. Keeping the first also makes the latch cheap to hit repeatedly.
    pub fn record(&self, err: impl std::fmt::Display) {
        let mut g = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if g.is_none() {
            *g = Some(err.to_string());
        }
    }

    /// Whether a fault has been latched.
    pub fn is_faulted(&self) -> bool {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).is_some()
    }

    /// The latched message, if any.
    pub fn message(&self) -> Option<String> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// `Err` once a fault has been latched, so a read/write reports device loss instead of returning
    /// empty data forever. Stays `Err` until [`StreamFault::clear`] — a fault is a property of the
    /// stream, and the stream does not heal itself.
    pub fn check(&self) -> Result<(), AudioError> {
        match self.message() {
            Some(m) => Err(AudioError::Stream(m)),
            None => Ok(()),
        }
    }

    /// Clear the latch. For a caller that has re-acquired the device and wants to reuse the latch.
    pub fn clear(&self) {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_latch_is_ok() {
        let f = StreamFault::new();
        assert!(!f.is_faulted());
        assert!(f.check().is_ok());
        assert_eq!(f.message(), None);
    }

    /// THE GATE: once the driver reports device loss, a reader must see an error rather than an
    /// unbroken run of successful empty reads.
    #[test]
    fn a_recorded_fault_makes_check_fail_and_stay_failed() {
        let f = StreamFault::new();
        f.record("device disconnected");

        assert!(f.is_faulted());
        let err = f.check().expect_err("check must report the fault");
        assert!(
            err.to_string().contains("device disconnected"),
            "the error must carry the driver's message, not a generic one: {err}"
        );
        // A fault is a property of the stream: re-checking must not clear it.
        assert!(
            f.check().is_err(),
            "the latch must stay failed — a stream does not heal itself"
        );
    }

    /// The first error names the cause; later ones are consequences of the same failure.
    #[test]
    fn the_first_error_wins() {
        let f = StreamFault::new();
        f.record("ALSA: device disconnected");
        f.record("stream closed");
        f.record("stream closed");
        assert_eq!(f.message().as_deref(), Some("ALSA: device disconnected"));
    }

    /// The callback and the reader hold clones; they must observe the same latch.
    #[test]
    fn clones_share_the_latch() {
        let callback_side = StreamFault::new();
        let reader_side = callback_side.clone();

        assert!(reader_side.check().is_ok());
        callback_side.record("xrun: device removed");
        assert!(
            reader_side.check().is_err(),
            "a fault recorded by the driver callback must be visible to the reader"
        );
    }

    #[test]
    fn clear_allows_reuse_after_reacquisition() {
        let f = StreamFault::new();
        f.record("device disconnected");
        assert!(f.check().is_err());
        f.clear();
        assert!(f.check().is_ok(), "a cleared latch is reusable");
        assert!(!f.is_faulted());
    }

    /// A poisoned lock must not wedge fault reporting: the panic that poisoned it is exactly the
    /// situation in which the fault most needs to be readable.
    #[test]
    fn a_poisoned_latch_still_reports() {
        let f = StreamFault::new();
        f.record("device disconnected");
        let f2 = f.clone();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = f2.0.lock().unwrap();
            panic!("poison the latch");
        }));
        assert!(
            f.check().is_err(),
            "fault must remain readable through a poisoned lock"
        );
    }
}
