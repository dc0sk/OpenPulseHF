//! The carrier detector must measure the PRE-AGC level, so an enabled AGC's boost can't wedge the squelch.
//!
//! Bug: DCD used to run on the samples returned by the InputCapture seam — i.e. POST-AGC. After a weak
//! burst ramps the AGC gain up (and the active-span gate freezes it there through silence), the held gain
//! multiplies sub-squelch band noise back over the DCD busy threshold, so the channel reads "busy" forever
//! and CSMA never lets the station transmit. DCD now runs inside the seam, before the AGC, on the true
//! channel level. This pins that the boosted-noise floor no longer registers as a carrier.

use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;
use qpsk_plugin::QpskPlugin;

const MODE: &str = "QPSK500";

fn lcg(seed: &mut u64) -> f32 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    ((*seed >> 40) as f32) / ((1u64 << 24) as f32) * 2.0 - 1.0
}

#[test]
fn agc_boost_does_not_leak_into_the_squelch() {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    e.register_plugin(Box::new(QpskPlugin::new())).unwrap();
    e.configure_agc(0.3, 0.05, 40.0);
    e.enable_agc();

    // A weak but carrier-present burst ramps the AGC gain up.
    let weak: Vec<f32> = (0..4096)
        .map(|i| 0.02 * (2.0 * std::f32::consts::PI * 1500.0 * i as f32 / 8000.0).sin())
        .collect();
    for _ in 0..60 {
        let _ = e.accumulate_capture(Some(MODE), weak.clone());
    }
    assert!(
        e.agc_gain_db() > 6.0,
        "precondition: the weak burst should have boosted the AGC gain, got {:.1} dB",
        e.agc_gain_db()
    );

    // Now feed low-level noise well below the DCD squelch. Boosted by the held gain (~26 dB) this would,
    // pre-fix, read well above the busy threshold.
    let mut seed = 42u64;
    for _ in 0..30 {
        let noise: Vec<f32> = (0..4096).map(|_| 0.0015 * lcg(&mut seed)).collect();
        let _ = e.accumulate_capture(Some(MODE), noise);
    }

    // DCD must report the TRUE (pre-AGC) energy of the last block — the ~0.001 RMS noise floor, not the
    // AGC-boosted ~0.02+. This is the deterministic discriminator: pre-fix DCD measured the post-AGC
    // samples so it read the boosted floor and latched busy forever; post-fix it reads the real level and
    // the busy flag expires with its hold window instead of being re-armed every noise block.
    // (`is_channel_busy()` itself is a wall-clock hold and can't be asserted in a sub-100 ms test — the
    // weak burst's own hold has not expired yet — so the energy is the sound, time-independent check.)
    assert!(
        e.dcd_energy() < 0.01,
        "DCD reported {:.4} — the AGC boost leaked into the squelch (pre-AGC noise is ~0.001 RMS)",
        e.dcd_energy()
    );
    // Tripwire: DCD actually ran at the seam on the daemon (accumulate_capture) path.
    assert!(
        e.dcd_blocks_processed() > 0,
        "DCD never ran at the InputCapture seam on the accumulate path"
    );
}
