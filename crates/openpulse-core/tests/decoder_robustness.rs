//! Decoder robustness against malformed input (audit 2026-07-19, finding #15).
//!
//! Every decoder here parses bytes that arrive from an **unauthenticated** source — off the air, or
//! off a TCP port with no authentication (REQ-SEC-CTL-06). A panic in any of them is a remote crash
//! of an unattended station, and prior hand-audits found real defects in exactly this code
//! (unbounded allocations from wire-supplied lengths, index-out-of-range on truncated frames).
//! There was no fuzzing, proptest or corpus anywhere in the tree.
//!
//! **Why not `cargo-fuzz`.** libFuzzer needs nightly and a separate `cargo fuzz run` invocation. The
//! CI workflow is disabled by choice and gates run locally, so a nightly-only target would never
//! execute — and a harness nobody runs finds nothing. This runs inside
//! `cargo test --workspace --no-default-features`, i.e. the gate that actually happens. It is a
//! smoke-level sweep, not a substitute for a real fuzzing campaign; `docs/dev/project/backlog.md`
//! carries the deeper campaign.
//!
//! **What is asserted.** Only that a decoder does not panic and does not hang: it must return, and
//! any `Ok` must be self-consistent. Correctness of *rejection* is the job of the targeted tests in
//! each crate; this covers the space those cannot enumerate.
//!
//! Determinism matters more than entropy here: a random seed would make a failure unreproducible, so
//! the PRNG is seeded from a fixed constant and every case is replayable from its index.

use openpulse_core::ack::AckFrame;
use openpulse_core::frame::Frame;
use openpulse_core::wire_query::{WireEnvelope, WireMsgType};

/// xorshift64*, so the corpus is reproducible without pulling in a PRNG dependency.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // Zero is a fixed point of xorshift; never let a caller land the generator on it.
        Self(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }
    fn byte(&mut self) -> u8 {
        (self.next_u64() >> 24) as u8
    }
    fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| self.byte()).collect()
    }
}

/// Mutations that take a *valid* encoding and damage it. Pure random bytes almost always die at the
/// first length or magic check; mutating a real frame is what reaches the deeper parsing where the
/// interesting defects live.
fn mutate(rng: &mut Rng, base: &[u8]) -> Vec<u8> {
    let mut out = base.to_vec();
    match rng.below(6) {
        // Flip a bit.
        0 if !out.is_empty() => {
            let i = rng.below(out.len());
            let bit = rng.below(8);
            out[i] ^= 1 << bit;
        }
        // Truncate — the classic "length field promises more than the buffer holds".
        1 if !out.is_empty() => {
            let keep = rng.below(out.len());
            out.truncate(keep);
        }
        // Extend with trailing garbage.
        2 => {
            let n = 1 + rng.below(64);
            out.extend(rng.bytes(n));
        }
        // Overwrite a run of bytes.
        3 if !out.is_empty() => {
            let at = rng.below(out.len());
            let room = out.len() - at;
            let n = 1 + rng.below(room);
            for b in out.iter_mut().skip(at).take(n) {
                *b = rng.byte();
            }
        }
        // Corrupt the head, where magic and version usually sit.
        4 if !out.is_empty() => {
            let n = out.len().min(8);
            for b in out.iter_mut().take(n) {
                *b = rng.byte();
            }
        }
        // Splice the frame against itself.
        _ => {
            let half = out.len() / 2;
            let tail = out.split_off(half);
            out.extend(tail.iter().rev());
        }
    }
    out
}

const CASES: usize = 4_000;

/// `Frame` carries every HPX payload off the air; it is the single most-reachable decoder.
#[test]
fn frame_decode_survives_mutated_and_random_input() {
    let mut rng = Rng::new(0xF00D_1234);
    let valid = Frame::new(7, b"the quick brown fox".to_vec())
        .expect("base frame")
        .encode();

    for i in 0..CASES {
        let input = if i % 2 == 0 {
            mutate(&mut rng, &valid)
        } else {
            let n = rng.below(512);
            rng.bytes(n)
        };
        // A panic here fails the test; that is the assertion. An Ok must be self-consistent.
        if let Ok(f) = Frame::decode(&input) {
            assert!(
                f.payload.len() <= input.len(),
                "case {i}: decoded payload ({} B) exceeds its own input ({} B) — a length field was \
                 trusted over the buffer",
                f.payload.len(),
                input.len()
            );
        }
    }
}

/// The OTA rate-control ACK: 5 bytes, decoded before any authentication decision.
#[test]
fn ack_decode_survives_every_five_byte_input() {
    let mut rng = Rng::new(0xACE0_5555);
    for _ in 0..CASES {
        let b = rng.bytes(5);
        let arr: [u8; 5] = [b[0], b[1], b[2], b[3], b[4]];
        let _ = AckFrame::decode(&arr);
        let _ = AckFrame::decode_authenticated(&arr, &[0u8; 32]);
    }
    // Exhaustively cover the low 16 bits of the space with the rest held at zero, so the sweep is
    // not purely probabilistic in the region most likely to hit type/flag dispatch.
    for v in 0u32..=0xFFFF {
        let arr = [(v >> 8) as u8, v as u8, 0, 0, 0];
        let _ = AckFrame::decode(&arr);
    }
}

/// `WireEnvelope` fronts peer-query, route-discovery and relay payloads on the mesh.
#[test]
fn wire_envelope_decode_survives_mutated_and_random_input() {
    let mut rng = Rng::new(0x5EED_9001);
    let valid = WireEnvelope {
        msg_type: WireMsgType::PeerQueryRequest,
        flags: 0,
        session_id: 1,
        src_peer_id: [0u8; 32],
        dst_peer_id: [1u8; 32],
        nonce: [2u8; 12],
        timestamp_ms: 0,
        hop_limit: 4,
        hop_index: 0,
        payload: vec![0xAB; 32],
        signature: None,
    }
    .encode()
    .expect("base envelope");

    for i in 0..CASES {
        let input = if i % 3 == 0 {
            let n = rng.below(400);
            rng.bytes(n)
        } else {
            mutate(&mut rng, &valid)
        };
        if let Ok(env) = WireEnvelope::decode(&input) {
            assert!(
                env.payload.len() <= input.len(),
                "case {i}: envelope payload ({} B) exceeds its own input ({} B)",
                env.payload.len(),
                input.len()
            );
        }
    }
}

/// Empty and single-byte inputs are the cheapest crash class and the easiest to forget.
#[test]
fn degenerate_inputs_do_not_panic() {
    assert!(Frame::decode(&[]).is_err());
    assert!(WireEnvelope::decode(&[]).is_err());
    for b in 0u8..=255 {
        let _ = Frame::decode(&[b]);
        let _ = WireEnvelope::decode(&[b]);
    }
    // Length-prefix helpers sit directly under the demodulator, on LLR slices of arbitrary length.
    for n in 0..64 {
        let _ = openpulse_core::len_prefix::decode_len_prefix(&vec![0xFFu8; n]);
        let _ = openpulse_core::len_prefix::decode_len_prefix_llrs(&vec![0.0f32; n]);
        let _ = openpulse_core::len_prefix::decode_len_prefix_llrs(&vec![f32::NAN; n]);
    }
}

/// The harness must actually reach the decoders. If a mutation strategy or constructor changed such
/// that every input died at the first byte, the sweeps above would pass while testing nothing —
/// the vacuous-gate failure this repo keeps finding.
#[test]
fn the_sweep_actually_reaches_the_decoders() {
    let mut rng = Rng::new(0xF00D_1234);
    let valid = Frame::new(7, b"the quick brown fox".to_vec())
        .expect("base frame")
        .encode();
    assert!(
        Frame::decode(&valid).is_ok(),
        "the 'valid' base frame does not decode — the corpus is built on a broken sample, so every \
         mutation is testing the reject path only"
    );

    let mut accepted = 0usize;
    let mut distinct_lengths = std::collections::BTreeSet::new();
    for _ in 0..CASES {
        let input = mutate(&mut rng, &valid);
        distinct_lengths.insert(input.len());
        if Frame::decode(&input).is_ok() {
            accepted += 1;
        }
    }
    assert!(
        distinct_lengths.len() > 20,
        "mutations produced only {} distinct lengths — the mutator is not exploring",
        distinct_lengths.len()
    );
    assert!(
        accepted > 0,
        "not one mutated frame decoded successfully across {CASES} cases: the sweep never gets past \
         the header, so it is not exercising the payload path at all"
    );
}
