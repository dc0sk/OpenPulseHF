//! How long to wait for a transmit buffer to drain before declaring the driver stuck.
//!
//! **Why this exists.** The deadline was computed inline as
//! `(queued_seconds + 3.0).clamp(5.0, 60.0)`, under a comment that read "Timeout adapts to queued
//! audio length so slow modes can fully drain". The adaptation was right; the **upper clamp made it
//! inert for exactly those slow modes**. Reed–Solomon pads any payload to a full 255-byte block, so a
//! `BPSK31` frame is 255 × 8 ÷ 31.25 = **65.3 s** of audio. It asked for 68.3 s, was clamped to 60 s,
//! and failed 100 % of the time with `output buffer did not drain within 60.0 s` — the mode could not
//! transmit at all on real hardware, and it is `hpx_hf` SL2.
//!
//! The invariant that was missing: **the deadline must always exceed the audio it is waiting on**,
//! or it is guaranteed to fire on a perfectly healthy driver.
//!
//! This lives in an **ungated** module, not behind `cpal-backend`, for the same reason as
//! [`crate::fault`]: the workspace suite runs `--no-default-features`, so logic inside the cpal module
//! is untestable in the gate that actually runs.

/// Slack over the queued audio duration, as a fraction, to absorb driver scheduling jitter.
const SLACK_FRACTION: f64 = 0.25;

/// Fixed slack added on top, covering hardware-buffer drain and short-queue overhead.
const SLACK_SECONDS: f64 = 5.0;

/// Floor, so a nearly-empty queue still tolerates a slow scheduler.
const MIN_TIMEOUT_SECONDS: f64 = 5.0;

/// Runaway backstop. Far above any real frame (the slowest, `BPSK31` at a full three-block payload,
/// is ~196 s), so it never binds in practice — it exists only to bound a pathological queue length.
const MAX_TIMEOUT_SECONDS: f64 = 600.0;

/// Seconds to wait for `queued_samples` to drain at `sample_rate_hz` × `channels`.
///
/// Always returns more than the queued audio's own duration, up to [`MAX_TIMEOUT_SECONDS`].
pub fn flush_timeout_seconds(queued_samples: usize, sample_rate_hz: u32, channels: u16) -> f64 {
    let samples_per_second = f64::from(sample_rate_hz) * f64::from(channels);
    let queued_seconds = if samples_per_second > 0.0 {
        queued_samples as f64 / samples_per_second
    } else {
        0.0
    };
    (queued_seconds * (1.0 + SLACK_FRACTION) + SLACK_SECONDS)
        .clamp(MIN_TIMEOUT_SECONDS, MAX_TIMEOUT_SECONDS)
}

/// Ceiling on the wait after the queue empties: the fixed sleep it replaces (#1367), so the wait is
/// never longer than it was.
pub(crate) const DRAIN_WAIT_CAP: std::time::Duration = std::time::Duration::from_millis(200);

/// Margin over the device's own report, for the part of the output path it does not count (a USB
/// codec's FIFO and DAC, about 1–2 ms).
pub(crate) const DRAIN_MARGIN: std::time::Duration = std::time::Duration::from_millis(10);

/// How long after the output callback the last queued sample leaves the device, or `None` when the
/// report cannot be trusted.
///
/// `delay` is cpal's `playback − callback`: on ALSA, the frames already queued in the hardware when
/// the callback ran. `last_frame` is the 1-based frame position of the last real sample in that
/// callback's buffer. A delay of ZERO is not trusted: cpal clamps an underrun's negative delay to 0,
/// and a running stream whose callback fired always has something queued — so 0 is the report that
/// lies, and it would release the transmitter with a whole hardware buffer still playing.
pub fn drain_after_callback(
    delay: Option<std::time::Duration>,
    last_frame: usize,
    sample_rate_hz: u32,
) -> Option<std::time::Duration> {
    let delay = delay.filter(|d| !d.is_zero())?;
    (sample_rate_hz > 0).then(|| {
        delay + std::time::Duration::from_secs_f64(last_frame as f64 / f64::from(sample_rate_hz))
    })
}

/// The wait once the software queue is empty: until the device's reported drain plus
/// [`DRAIN_MARGIN`], never above [`DRAIN_WAIT_CAP`]; the full cap when there is no trustworthy report.
pub fn drain_wait(remaining: Option<std::time::Duration>) -> std::time::Duration {
    remaining.map_or(DRAIN_WAIT_CAP, |r| (r + DRAIN_MARGIN).min(DRAIN_WAIT_CAP))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Samples for `seconds` of 8 kHz mono audio — the modem's transmit format.
    fn samples_for(seconds: f64) -> usize {
        (seconds * 8000.0) as usize
    }

    /// THE GATE: a `BPSK31` + RS frame must get a deadline longer than its own airtime.
    ///
    /// Before the fix this returned 60.0 for 65.3 s of audio — guaranteed failure on a healthy driver.
    #[test]
    fn a_bpsk31_rs_frame_gets_longer_than_its_own_airtime() {
        // RS pads to one full 255-byte block; BPSK31 is 31.25 baud, 1 bit/symbol.
        let airtime = 255.0 * 8.0 / 31.25;
        assert!(
            (65.0..66.0).contains(&airtime),
            "airtime arithmetic drifted: {airtime}"
        );

        let timeout = flush_timeout_seconds(samples_for(airtime), 8000, 1);
        assert!(
            timeout > airtime,
            "a {airtime:.1} s frame got a {timeout:.1} s deadline — the wait is shorter than the \
             audio, so a healthy driver is guaranteed to time out"
        );
    }

    /// The invariant in general: never promise less time than the audio takes to play.
    #[test]
    fn the_deadline_always_exceeds_the_queued_audio() {
        for seconds in [0.0, 0.5, 5.0, 30.0, 60.0, 65.3, 120.0, 196.0, 400.0] {
            let timeout = flush_timeout_seconds(samples_for(seconds), 8000, 1);
            assert!(
                timeout > seconds,
                "{seconds} s of queued audio got a {timeout} s deadline"
            );
        }
    }

    /// The slowest frame the ladder can produce — `BPSK31` at a three-block payload — still fits.
    #[test]
    fn the_slowest_possible_frame_fits_under_the_backstop() {
        let airtime = 3.0 * 255.0 * 8.0 / 31.25; // ~195.8 s
        let timeout = flush_timeout_seconds(samples_for(airtime), 8000, 1);
        assert!(
            timeout > airtime,
            "the slowest ladder frame ({airtime:.1} s) exceeds its {timeout:.1} s deadline"
        );
        assert!(timeout <= MAX_TIMEOUT_SECONDS);
    }

    /// A short queue keeps a sane floor rather than a near-zero deadline.
    #[test]
    fn a_short_queue_keeps_the_floor() {
        assert_eq!(flush_timeout_seconds(0, 8000, 1), MIN_TIMEOUT_SECONDS);
        // 10 ms of audio: above the floor by its own slack, never below it.
        let t = flush_timeout_seconds(80, 8000, 1);
        assert!(t >= MIN_TIMEOUT_SECONDS, "{t} fell below the floor");
        assert!(
            t < MIN_TIMEOUT_SECONDS + 1.0,
            "{t} is implausibly long for 10 ms"
        );
    }

    /// Stereo and higher sample rates halve/scale the duration, not just the sample count.
    #[test]
    fn channel_count_and_rate_scale_the_duration() {
        let mono = flush_timeout_seconds(48_000, 48_000, 1); // 1.0 s
        let stereo = flush_timeout_seconds(48_000, 48_000, 2); // 0.5 s
        assert!(mono >= stereo);
        assert!(flush_timeout_seconds(480_000, 48_000, 1) > 10.0);
    }

    /// A zero sample rate must not divide by zero or return a NaN deadline.
    #[test]
    fn a_zero_sample_rate_falls_back_to_the_floor() {
        let t = flush_timeout_seconds(1000, 0, 1);
        assert!(t.is_finite());
        assert_eq!(t, MIN_TIMEOUT_SECONDS);
    }

    use std::time::Duration;

    #[test]
    fn a_reported_delay_sets_the_wait() {
        let after = drain_after_callback(Some(Duration::from_millis(20)), 80, 8000);
        assert_eq!(
            after,
            Some(Duration::from_millis(30)),
            "20 ms queued + 80 frames at 8 kHz"
        );
        assert_eq!(
            drain_wait(after),
            Duration::from_millis(40),
            "plus the 10 ms margin"
        );
    }

    /// THE #1367 REVIEW FINDING: cpal reports an underrun as delay 0. Trusting it would release the
    /// transmitter with a whole hardware buffer still playing.
    #[test]
    fn a_zero_delay_is_not_trusted_and_falls_back_to_the_full_wait() {
        assert_eq!(drain_after_callback(Some(Duration::ZERO), 80, 8000), None);
        assert_eq!(drain_wait(None), DRAIN_WAIT_CAP);
    }

    #[test]
    fn no_report_or_no_rate_falls_back_to_the_full_wait() {
        assert_eq!(drain_after_callback(None, 80, 8000), None);
        assert_eq!(
            drain_after_callback(Some(Duration::from_millis(20)), 80, 0),
            None
        );
    }

    #[test]
    fn the_wait_is_never_longer_than_before() {
        let after = drain_after_callback(Some(Duration::from_millis(500)), 0, 8000);
        assert_eq!(drain_wait(after), DRAIN_WAIT_CAP);
    }
}
