//! The plain 8PSK pulse needs ≥5 samples/symbol, and says so instead of emitting undecodable audio.
//!
//! **The defect this pins.** `samples_per_symbol` enforced a floor of 4, so `8PSK2000` at 8 kHz (4
//! samples/symbol) was accepted, modulated, and transmitted — and nothing could decode it. The
//! receiver reported a generic framing error, which reads as a channel problem rather than a
//! configuration that cannot work. It surfaced as a `FAIL` on both the virtual and the dual-card
//! loopback rungs, and reproduced in-process on a **clean, noiseless** channel — the repo's signature
//! for a bug rather than a limitation.
//!
//! **Why 5 and not 4.** The plain modulator blends adjacent symbols with a raised cosine and the
//! demodulator integrates against the squared window, leaving a residual ISI term that grows as `n`
//! shrinks (β ≈ 0.182 at 16 sps, 0.167 at 8 sps — CLAUDE.md → *Known sharp edges*). At 4 sps it
//! exceeds 8PSK's ±22.5° decision margin. Measured 2026-07-20, all on a clean channel:
//!
//! | mode | sps | pulse | round-trips |
//! |---|---|---|---|
//! | `8PSK500` | 16 | plain | yes |
//! | `8PSK1000` | 8 | plain | yes |
//! | `8PSK9600` @ 48 kHz | 5 | plain | yes |
//! | `8PSK2000` | 4 | plain | **no** |
//! | `8PSK2000-RRC` | 4 | RRC | yes |
//! | `QPSK2000` | 4 | plain | yes |
//!
//! The floor is therefore **5**, not 8. The first version of this guard used 8, generalised from the
//! 4/8/16 samples the 8 kHz modes happen to give — straight past the boundary that made it true. The
//! pre-existing `psk8_9600_loopback_48k` test caught it. See also the `RsStrong is free` entry in
//! CLAUDE.md, which is the same mistake.
//!
//! So it is the **phase margin** that runs out, not the sample rate: QPSK has ±45° to spend at the
//! same 4 sps and 8PSK does not. This is the same ordering the whole codebase follows.
//!
//! The mode itself is legitimate — at 48 kHz `8PSK2000` is 24 samples/symbol and fine. Only the
//! combination with 8 kHz is refused, which is why the plugin still advertises the mode.

use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use psk8_plugin::Psk8Plugin;

fn config(mode: &str, sample_rate: u32) -> ModulationConfig {
    ModulationConfig {
        mode: mode.to_string(),
        center_frequency: 1500.0,
        sample_rate,
        ..ModulationConfig::default()
    }
}

/// THE GATE: the plain pulse at 4 sps is refused, with a message that names the way out.
#[test]
fn plain_8psk_at_four_samples_per_symbol_is_refused_with_a_usable_message() {
    let plugin = Psk8Plugin::new();
    let err = plugin
        .modulate(&[1, 2, 3, 4], &config("8PSK2000", 8000))
        .expect_err("8PSK2000 at 8 kHz is 4 samples/symbol and cannot decode; it must be refused");

    let msg = err.to_string();
    assert!(
        msg.contains("samples/symbol"),
        "the error must explain the rate constraint, got: {msg}"
    );
    assert!(
        msg.contains("8PSK2000-RRC"),
        "the error must point at the variant that does work at this rate, got: {msg}"
    );
}

/// The receiver refuses it too, rather than scanning for a frame that could never be sent.
#[test]
fn the_receiver_refuses_the_same_combination() {
    let plugin = Psk8Plugin::new();
    let silence = vec![0.0f32; 8000];
    let err = plugin
        .demodulate(&silence, &config("8PSK2000", 8000))
        .expect_err("the demodulator must refuse a mode/rate pair the modulator cannot produce");
    assert!(err.to_string().contains("samples/symbol"), "{err}");
}

/// Control: the rungs that DO work must keep working — this guard must not cost a usable mode.
#[test]
fn the_working_plain_modes_are_unaffected() {
    let plugin = Psk8Plugin::new();
    for mode in ["8PSK500", "8PSK1000"] {
        assert!(
            plugin.modulate(&[1, 2, 3, 4], &config(mode, 8000)).is_ok(),
            "{mode} has >=5 samples/symbol at 8 kHz and must still modulate"
        );
    }
}

/// The boundary itself: 5 samples/symbol works with the plain pulse and must not be refused.
///
/// This is the case that refuted a floor of 8. Without it the guard silently costs a working mode.
#[test]
fn the_plain_pulse_is_accepted_at_exactly_five_samples_per_symbol() {
    let plugin = Psk8Plugin::new();
    let cfg = ModulationConfig {
        center_frequency: 12000.0,
        ..config("8PSK9600", 48000)
    };
    let payload = b"8PSK9600 at 5 sps";
    let tx = plugin
        .modulate(payload, &cfg)
        .expect("8PSK9600 at 48 kHz is 5 samples/symbol and round-trips; it must not be refused");
    let rx = plugin.demodulate(&tx, &cfg).expect("demodulate");
    assert_eq!(&rx[..payload.len()], payload, "5 sps must still decode");
}

/// Control: the RRC variant is a matched filter and works at the very rate the plain pulse cannot.
#[test]
fn the_rrc_variant_still_works_at_four_samples_per_symbol() {
    let plugin = Psk8Plugin::new();
    assert!(
        plugin
            .modulate(&[1, 2, 3, 4], &config("8PSK2000-RRC", 8000))
            .is_ok(),
        "8PSK2000-RRC is shaped and must remain usable at 4 samples/symbol"
    );
}

/// The refusal is about the rate, not the mode: raise the sample rate and `8PSK2000` is fine.
#[test]
fn the_same_mode_is_accepted_at_a_sample_rate_that_gives_enough_symbols() {
    let plugin = Psk8Plugin::new();
    assert!(
        plugin
            .modulate(&[1, 2, 3, 4], &config("8PSK2000", 48000))
            .is_ok(),
        "8PSK2000 at 48 kHz is 24 samples/symbol; refusing it would be wrong"
    );
}
