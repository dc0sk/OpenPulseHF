//! LDPC / Turbo code preparation (BL-FEC-6).
//!
//! This module defines the `IterativeDecoder` trait that any future iterative
//! FEC implementation (LDPC belief-propagation, Turbo BCJR) must satisfy, and
//! provides a stub `LdpcCodec` that compiles and type-checks but returns an
//! error on use.
//!
//! # Why deferred
//!
//! Practical LDPC decoding requires ≥ 50 belief-propagation iterations over a
//! sparse parity-check matrix of ≥ 1 000 bits.  Each iteration is a dense
//! message-passing pass with unpredictable convergence; CPU latency is
//! incompatible with the fixed 1.25 s HPX ARQ cycle budget.  The planned path
//! is wgpu compute shaders in `crates/openpulse-gpu`, once that crate matures.

use crate::error::ModemError;

// ── Trait ─────────────────────────────────────────────────────────────────────

/// Shared interface for iterative FEC codecs (LDPC, Turbo).
///
/// All methods operate on whole blocks; callers must split larger payloads.
pub trait IterativeDecoder: Send + Sync {
    /// Encode `data` bytes and return the codeword (data + parity).
    fn encode(&self, data: &[u8]) -> Vec<u8>;

    /// Soft-decision decode `llrs` (one `f32` per coded bit, positive = likely 0)
    /// and return the recovered data bytes.
    ///
    /// Returns `Err` if the decoder fails to converge within `max_iterations()`.
    fn decode_soft(&self, llrs: &[f32]) -> Result<Vec<u8>, ModemError>;

    /// Maximum belief-propagation (or BCJR) iterations before declaring failure.
    fn max_iterations(&self) -> u32;

    /// Size of one information block in bits (before encoding).
    fn block_bits(&self) -> usize;
}

// ── Stub ──────────────────────────────────────────────────────────────────────

/// Placeholder LDPC codec.  Not yet implemented — returns `Err` on all calls.
///
/// Exists so `FecMode::Ldpc` can be wired into the engine dispatch table before
/// the GPU acceleration path (`openpulse-gpu`) is ready.
pub struct LdpcCodec;

impl IterativeDecoder for LdpcCodec {
    fn encode(&self, _data: &[u8]) -> Vec<u8> {
        // Will be replaced by a proper LDPC encoder once the wgpu path lands.
        unimplemented!("LDPC encode is not yet implemented (BL-FEC-6 deferred)")
    }

    fn decode_soft(&self, _llrs: &[f32]) -> Result<Vec<u8>, ModemError> {
        Err(ModemError::Demodulation(
            "LDPC soft decode not yet implemented (BL-FEC-6 deferred — requires GPU)".into(),
        ))
    }

    fn max_iterations(&self) -> u32 {
        50
    }

    fn block_bits(&self) -> usize {
        1024
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ldpc_stub_decode_returns_err() {
        let codec = LdpcCodec;
        assert!(codec.decode_soft(&[1.0f32; 2048]).is_err());
    }

    #[test]
    fn ldpc_stub_metadata() {
        let codec = LdpcCodec;
        assert_eq!(codec.max_iterations(), 50);
        assert_eq!(codec.block_bits(), 1024);
    }
}
