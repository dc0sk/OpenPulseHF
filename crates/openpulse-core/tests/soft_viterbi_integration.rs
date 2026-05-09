//! BL-FEC-5 integration tests: coding-gain verification for K=7 soft Viterbi.

use openpulse_core::conv::ConvCodec;
use openpulse_core::soft_viterbi::SoftViterbiCodec;

/// Inject `error_rate` random bit errors into `encoded` using a fixed LCG seed.
/// Returns the corrupted bytes.
fn corrupt_bytes(encoded: &[u8], error_rate: f64, seed: u64) -> Vec<u8> {
    let mut out = encoded.to_vec();
    let mut state = seed;
    for byte in &mut out {
        for bit in 0..8u8 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let r = (state >> 11) as f64 / (1u64 << 53) as f64;
            if r < error_rate {
                *byte ^= 1 << bit;
            }
        }
    }
    out
}

/// Convert encoded bytes to soft LLRs at a given SNR (Gaussian noise via Box-Muller).
fn bytes_to_llrs_noisy(encoded: &[u8], snr_linear: f32, seed: u64) -> Vec<f32> {
    let mut state = seed;
    let noise_std = (0.5_f32 / snr_linear).sqrt(); // BPSK: Eb/N0 = snr_linear/2

    let bits: Vec<f32> = encoded
        .iter()
        .flat_map(|&b| (0..8u8).map(move |i| if (b >> i) & 1 == 0 { 1.0f32 } else { -1.0f32 }))
        .collect();

    bits.iter()
        .map(|&s| {
            // Box-Muller Gaussian sample
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let u1 = ((state >> 11) as f32 / (1u64 << 53) as f32).max(1e-12);
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let u2 = (state >> 11) as f32 / (1u64 << 53) as f32;
            let noise = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f32::consts::PI * u2).cos();
            s + noise_std * noise
        })
        .collect()
}

/// Unpack encoded bytes to hard bits (LSB-first), then convert to ±1 soft values.
fn hard_llrs(encoded: &[u8]) -> Vec<f32> {
    encoded
        .iter()
        .flat_map(|&b| (0..8u8).map(move |i| if (b >> i) & 1 == 0 { 1.0f32 } else { -1.0f32 }))
        .collect()
}

fn count_bit_errors(a: &[u8], b: &[u8]) -> usize {
    a.iter()
        .zip(b.iter())
        .map(|(&x, &y)| (x ^ y).count_ones() as usize)
        .sum()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[test]
fn k7_encode_decode_round_trip() {
    let codec = SoftViterbiCodec;
    let payload: Vec<u8> = (0u8..64).collect();
    let encoded = codec.encode(&payload);
    let llrs = hard_llrs(&encoded);
    let decoded = codec.decode_soft(&llrs).unwrap();
    assert_eq!(decoded, payload);
}

#[test]
fn k7_soft_vs_hard_5pct_ber() {
    // At 5% raw BER, K=7 soft Viterbi should produce far fewer errors than K=3 hard.
    let payload: Vec<u8> = (0u8..128).collect();

    let k7 = SoftViterbiCodec;
    let k3 = ConvCodec::new();

    let enc_k7 = k7.encode(&payload);
    let enc_k3 = k3.encode(&payload);

    let corrupt_k7 = corrupt_bytes(&enc_k7, 0.05, 0xDEAD_BEEF);
    let corrupt_k3 = corrupt_bytes(&enc_k3, 0.05, 0xDEAD_BEEF);

    // K=7 soft: use ±1.0 LLRs from the corrupted encoded bytes.
    let llrs_k7: Vec<f32> = corrupt_k7
        .iter()
        .flat_map(|&b| (0..8u8).map(move |i| if (b >> i) & 1 == 0 { 1.0f32 } else { -1.0f32 }))
        .collect();
    let decoded_k7 = k7.decode_soft(&llrs_k7).unwrap_or_default();

    // K=3 hard: direct decode from corrupted bytes.
    let decoded_k3 = k3.decode(&corrupt_k3).unwrap_or_default();

    let errors_k7 = count_bit_errors(&decoded_k7, &payload);
    let errors_k3 = count_bit_errors(
        &decoded_k3[..decoded_k3.len().min(payload.len())],
        &payload[..decoded_k3.len().min(payload.len())],
    );

    assert!(
        errors_k7 < errors_k3,
        "K=7 soft ({errors_k7} bit errors) should beat K=3 hard ({errors_k3} bit errors) at 5% BER"
    );
}

#[test]
fn k7_coding_gain_over_uncoded() {
    // At 2% raw BER, uncoded data is heavily corrupted; K=7 should recover perfectly.
    let payload: Vec<u8> = (0u8..64).collect();
    let k7 = SoftViterbiCodec;

    // Corrupt the raw payload at 2% BER.
    let corrupted_raw = corrupt_bytes(&payload, 0.02, 0xCAFE_BABE);
    let raw_errors = count_bit_errors(&corrupted_raw, &payload);

    // Encode, corrupt the encoded stream at 2% BER, soft-decode.
    let encoded = k7.encode(&payload);
    let corrupted_enc = corrupt_bytes(&encoded, 0.02, 0xCAFE_BABE);
    let llrs: Vec<f32> = corrupted_enc
        .iter()
        .flat_map(|&b| (0..8u8).map(move |i| if (b >> i) & 1 == 0 { 1.0f32 } else { -1.0f32 }))
        .collect();
    let decoded = k7.decode_soft(&llrs).unwrap_or_default();
    let fec_errors = count_bit_errors(&decoded, &payload);

    assert!(
        fec_errors < raw_errors,
        "K=7 FEC ({fec_errors} errors) should beat uncoded ({raw_errors} errors) at 2% BER"
    );
    assert_eq!(
        fec_errors, 0,
        "K=7 should recover perfectly at 2% BER with hard-decision soft values"
    );
}

#[test]
fn k7_soft_beats_hard_at_low_snr() {
    // At SNR=3dB, true soft-decision LLRs should outperform hard ±1.0 pseudo-LLRs.
    // We use many repetitions and measure total bit errors.
    let payload: Vec<u8> = (0u8..64).collect();
    let k7 = SoftViterbiCodec;
    let encoded = k7.encode(&payload);

    let snr_linear = 10.0_f32.powf(3.0 / 10.0); // 3 dB

    let mut errors_soft = 0usize;
    let mut errors_hard = 0usize;

    for trial in 0u64..20 {
        // Soft: Gaussian-noisy LLRs
        let llrs_soft = bytes_to_llrs_noisy(&encoded, snr_linear, trial * 0x1234_5678 + 1);
        let dec_soft = k7.decode_soft(&llrs_soft).unwrap_or_default();
        errors_soft += count_bit_errors(&dec_soft, &payload);

        // Hard: quantize the same noisy LLRs to ±1.0
        let llrs_hard: Vec<f32> = llrs_soft.iter().map(|&l| l.signum()).collect();
        let dec_hard = k7.decode_soft(&llrs_hard).unwrap_or_default();
        errors_hard += count_bit_errors(&dec_hard, &payload);
    }

    assert!(
        errors_soft <= errors_hard,
        "soft LLRs ({errors_soft} errors over 20 trials) should not be worse than hard ({errors_hard})"
    );
}
