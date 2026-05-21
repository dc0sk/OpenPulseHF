//! BER test for the rate-1/3 PCCC turbo codec.
//!
//! Acceptance criterion (backlog item 7): BER ≤ 0.01 at Eb/N0 = 2 dB for
//! 256-bit information blocks (K = 256, 3GPP QPP interleaver).

use openpulse_core::turbo::TurboCodec;
use std::f64::consts::PI;

// ── Deterministic Gaussian noise via LCG + Box-Muller ─────────────────────────

fn lcg_next(s: &mut u64) -> f64 {
    *s = s
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    (*s >> 11) as f64 / (1u64 << 53) as f64
}

fn box_muller(s: &mut u64) -> f64 {
    let u1 = lcg_next(s).max(1e-15);
    let u2 = lcg_next(s);
    (-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos()
}

/// Apply AWGN at the given Eb/N0 (linear) for a rate-1/3 turbo codeword.
/// Returns LLRs (positive = likely 0).
fn awgn_channel(encoded_bytes: &[u8], ebno_linear: f64, seed: &mut u64) -> Vec<f32> {
    let code_rate = 1.0_f64 / 3.0;
    let ecno = ebno_linear * code_rate;
    // σ² = N₀/2, N₀ = Eₛ/Ec/N₀ with Eₛ = 1
    let sigma_sq = 1.0 / (2.0 * ecno);
    let sigma = sigma_sq.sqrt();

    encoded_bytes
        .iter()
        .flat_map(|&b| (0..8u32).rev().map(move |i| (b >> i) & 1))
        .map(|bit| {
            let symbol = if bit == 0 { 1.0_f64 } else { -1.0_f64 };
            let noisy = symbol + box_muller(seed) * sigma;
            // LLR = 2r / σ²
            (2.0 * noisy / sigma_sq) as f32
        })
        .collect()
}

#[test]
fn turbo_ber_256_bit_block_at_2db_ebno() {
    let codec = TurboCodec::new();

    // 28-byte payload → frame = 2+28+2 = 32 bytes = 256 bits = K=256 QPP block.
    let payload: Vec<u8> = (0..28u8)
        .map(|i| i.wrapping_mul(37).wrapping_add(13))
        .collect();

    const REPS: usize = 200;
    const EBN0_DB: f64 = 2.0;
    let ebno = 10_f64.powf(EBN0_DB / 10.0);

    let mut total_bits = 0u64;
    let mut error_bits = 0u64;
    let mut seed = 0xDEAD_BEEF_1234_5678u64;

    for _ in 0..REPS {
        let encoded = codec.encode(&payload);
        let llrs = awgn_channel(&encoded, ebno, &mut seed);
        let decoded = codec.decode(&llrs).unwrap_or_default();

        total_bits += (payload.len() as u64) * 8;
        let cmp_len = payload.len().min(decoded.len());
        for (&a, &b) in payload[..cmp_len].iter().zip(decoded[..cmp_len].iter()) {
            error_bits += (a ^ b).count_ones() as u64;
        }
        error_bits += ((payload.len() - cmp_len) as u64) * 8;
    }

    let ber = error_bits as f64 / total_bits as f64;
    println!(
        "Turbo BER at Eb/N0 = {EBN0_DB} dB: {ber:.6} ({error_bits} errors / {total_bits} bits)"
    );
    assert!(
        ber <= 0.01,
        "Turbo BER {ber:.6} exceeds 0.01 at Eb/N0 = {EBN0_DB} dB"
    );
}

#[test]
fn turbo_round_trip_noiseless() {
    let codec = TurboCodec::new();
    let payload = b"turbo noiseless";
    let encoded = codec.encode(payload);
    let llrs: Vec<f32> = encoded
        .iter()
        .flat_map(|&b| (0..8u32).rev().map(move |i| (b >> i) & 1))
        .map(|bit| if bit == 0 { 10.0f32 } else { -10.0f32 })
        .collect();
    let decoded = codec
        .decode(&llrs)
        .expect("noiseless round-trip must succeed");
    assert_eq!(decoded, payload);
}
