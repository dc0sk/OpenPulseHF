//! Fixture parameters shared between a gate and anything claiming to reproduce it.
//!
//! Included with `mod common;` from each test binary that needs it — integration tests are separate
//! crates and cannot import each other's items, so a shared source file is the only way to make
//! "this reproduction uses the gate's parameters" a claim the compiler checks.
//!
//! That is the point. `CLAUDE.md`'s verification mechanics ban a doc-comment fidelity claim over
//! hand-transcribed parameters by name, because a comment cannot fail — the origin story is a
//! harness that claimed to reproduce a QPSK500 gate while defaulting to QPSK1000, which inverted
//! the conclusion drawn from it. Every constant here is used by the gate itself, so a reproduction
//! that drifts stops compiling rather than quietly measuring something else.

#![allow(dead_code)]

/// The saturating-floor fixture: a recorded idle floor hot enough to clamp the energy gate, with a
/// frame embedded at a chosen lead. Used by `the_receiver_never_settles_on_a_saturating_noise_floor`
/// and by reproductions of it.
pub mod saturating_floor {
    /// Recorded idle capture whose floor saturates the energy gate.
    pub const CORPUS: &str = "ic9700-idle-hot.wav";
    /// Below this the capture no longer saturates the gate and the fixture's premise is gone.
    pub const GATE_CEILING_MEAN_SQ: f32 = 0.0032;
    /// Leads, in samples, at which the frame is embedded. The lead is the variable: a short one
    /// passes even on broken code because the recovery walk is short enough to finish.
    pub const LEADS: [usize; 3] = [40_000, 80_000, 120_000];
    /// Trailing idle after the frame.
    pub const TRAIL: usize = 40_000;
    /// Frame amplitude relative to the embedded capture.
    pub const EMBED_LEVEL: f32 = 0.3;
    /// Mode and FEC the fixture transmits.
    pub const MODE: &str = "BPSK250";
    /// Payload, also the expected decode.
    pub const PAYLOAD: &[u8] = b"correlation gate probe";
    /// Listen window.
    pub const TIMEOUT_MS: u64 = 40_000;
}
