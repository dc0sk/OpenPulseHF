//! Reed-Solomon forward error correction (FEC) codec.
//!
//! Provides transparent byte-level error correction that can be layered on top
//! of any modulation plugin.  The codec splits input data into 223-byte blocks,
//! appends 32 ECC bytes per block (GF(2^8), ECC_LEN = 32), and can correct up
//! to **16 byte errors per block** on the receive side.
//!
//! # Wire layout
//!
//! ```text
//! ┌─────────────────────┬──────────────────────────────────────────────────┐
//! │ original length (4B)│  data (padded to BLOCK_DATA multiples)          │
//! └─────────────────────┴──────────────────────────────────────────────────┘
//!       ↓  split into 223-byte chunks, RS-encode each
//! ┌──────────────┬──────────────────────────────────────────────────────────┐
//! │ block 0 (255B) │ block 1 (255B) │ … │ block N (255B)                  │
//! └──────────────┴──────────────────────────────────────────────────────────┘
//! ```

use reed_solomon::{Decoder, Encoder};

use crate::error::ModemError;

/// ECC bytes appended per 255-byte RS block.
pub const FEC_ECC_LEN: usize = 32;

/// Data bytes per RS block (255 − ECC_LEN).
const BLOCK_DATA: usize = 255 - FEC_ECC_LEN;

/// Total bytes per encoded RS block.
const BLOCK_TOTAL: usize = 255;

/// Byte width of the big-endian original-length prefix.
const PREFIX_LEN: usize = 4;

/// Reed-Solomon codec.
///
/// Construct with [`FecCodec::new`] or [`Default`].  The same codec instance
/// can be reused for multiple encode/decode operations.
pub struct FecCodec {
    encoder: Encoder,
    decoder: Decoder,
}

impl FecCodec {
    /// Create a new codec with the default ECC configuration (ECC_LEN = 32,
    /// corrects up to 16 byte errors per 255-byte block).
    pub fn new() -> Self {
        Self {
            encoder: Encoder::new(FEC_ECC_LEN),
            decoder: Decoder::new(FEC_ECC_LEN),
        }
    }

    /// Encode `data` into a sequence of RS-protected 255-byte blocks.
    ///
    /// A 4-byte big-endian length prefix is prepended before blocking so the
    /// decoder can strip trailing padding without side-channel information.
    pub fn encode(&self, data: &[u8]) -> Vec<u8> {
        let orig_len = data.len() as u32;

        // Prefix + data.
        let mut input = Vec::with_capacity(PREFIX_LEN + data.len());
        input.extend_from_slice(&orig_len.to_be_bytes());
        input.extend_from_slice(data);

        // Pad up to the next multiple of BLOCK_DATA.
        let padded_len = input.len().div_ceil(BLOCK_DATA) * BLOCK_DATA;
        input.resize(padded_len, 0u8);

        let n_blocks = input.len() / BLOCK_DATA;
        let mut out = Vec::with_capacity(n_blocks * BLOCK_TOTAL);

        for chunk in input.chunks(BLOCK_DATA) {
            let encoded = self.encoder.encode(chunk);
            out.extend_from_slice(&encoded);
        }

        out
    }

    /// Decode and error-correct a sequence of RS-protected 255-byte blocks.
    ///
    /// Returns the original bytes that were passed to [`encode`](Self::encode).
    ///
    /// # Errors
    ///
    /// Returns [`ModemError::Fec`] when:
    /// - The input length is not a multiple of 255.
    /// - A block has more than `ECC_LEN / 2 = 16` byte errors.
    /// - The embedded length prefix is inconsistent with decoded content.
    pub fn decode(&self, data: &[u8]) -> Result<Vec<u8>, ModemError> {
        if data.is_empty() || data.len() % BLOCK_TOTAL != 0 {
            return Err(ModemError::Fec(format!(
                "FEC data length {} is not a non-zero multiple of {BLOCK_TOTAL}",
                data.len()
            )));
        }

        let mut decoded = Vec::with_capacity(data.len() / BLOCK_TOTAL * BLOCK_DATA);

        for (i, block) in data.chunks(BLOCK_TOTAL).enumerate() {
            let corrected = self.decoder.correct(block, None).map_err(|e| {
                ModemError::Fec(format!("RS correction failed at block {i}: {e:?}"))
            })?;
            // The corrected buffer is BLOCK_TOTAL bytes; we only keep the data
            // portion (first BLOCK_DATA bytes) — the ECC bytes are trailing.
            decoded.extend_from_slice(&corrected[..BLOCK_DATA]);
        }

        // Read original length from 4-byte big-endian prefix.
        if decoded.len() < PREFIX_LEN {
            return Err(ModemError::Fec(
                "decoded data too short to contain length prefix".into(),
            ));
        }
        let orig_len = u32::from_be_bytes(decoded[..PREFIX_LEN].try_into().unwrap()) as usize;

        let end = PREFIX_LEN + orig_len;
        if decoded.len() < end {
            return Err(ModemError::Fec(format!(
                "decoded data shorter than expected: have {} bytes, need {end}",
                decoded.len()
            )));
        }

        Ok(decoded[PREFIX_LEN..end].to_vec())
    }
}

impl Default for FecCodec {
    fn default() -> Self {
        Self::new()
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_empty() {
        let codec = FecCodec::new();
        let enc = codec.encode(&[]);
        let dec = codec.decode(&enc).unwrap();
        assert!(dec.is_empty());
    }

    #[test]
    fn round_trip_small() {
        let codec = FecCodec::new();
        let payload = b"OpenPulse FEC test";
        let enc = codec.encode(payload);
        assert_eq!(
            enc.len() % BLOCK_TOTAL,
            0,
            "encoded length must be multiple of 255"
        );
        let dec = codec.decode(&enc).unwrap();
        assert_eq!(dec, payload);
    }

    #[test]
    fn round_trip_exact_block() {
        // Payload that fills exactly one block (BLOCK_DATA - PREFIX_LEN = 219 bytes).
        let payload = vec![0x42u8; BLOCK_DATA - PREFIX_LEN];
        let codec = FecCodec::new();
        let enc = codec.encode(&payload);
        assert_eq!(enc.len(), BLOCK_TOTAL);
        let dec = codec.decode(&enc).unwrap();
        assert_eq!(dec, payload);
    }

    #[test]
    fn round_trip_multi_block() {
        // Payload that requires multiple blocks.
        let payload: Vec<u8> = (0..500u16).map(|v| (v & 0xFF) as u8).collect();
        let codec = FecCodec::new();
        let enc = codec.encode(&payload);
        assert_eq!(enc.len() % BLOCK_TOTAL, 0);
        let dec = codec.decode(&enc).unwrap();
        assert_eq!(dec, payload);
    }

    #[test]
    fn corrects_up_to_16_byte_errors_per_block() {
        let codec = FecCodec::new();
        let payload = b"error correction test payload abc";
        let mut enc = codec.encode(payload);

        // Flip 16 bytes in the first block (max correctable per block).
        for i in 0..16 {
            enc[i * 4] ^= 0xFF;
        }

        let dec = codec.decode(&enc).unwrap();
        assert_eq!(
            dec, payload,
            "FEC should recover payload after ≤16 byte errors"
        );
    }

    #[test]
    fn fails_on_excessive_errors() {
        let codec = FecCodec::new();
        let payload = b"too many errors";
        let mut enc = codec.encode(payload);

        // Corrupt 17 bytes (one more than the correction capacity).
        for i in 0..17 {
            enc[i] ^= 0xFF;
        }

        assert!(
            codec.decode(&enc).is_err(),
            "should fail when errors exceed correction capacity"
        );
    }

    #[test]
    fn rejects_invalid_length() {
        let codec = FecCodec::new();
        assert!(codec.decode(&[0u8; 100]).is_err());
        assert!(codec.decode(&[]).is_err());
    }
}
