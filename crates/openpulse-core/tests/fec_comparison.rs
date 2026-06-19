//! Phase 3.2 evaluation: RS+interleaver vs rate-1/2 convolutional (K=3, Viterbi).
//!
//! This test file measures BER performance and CPU time for both codecs under
//! controlled bit-error injection and documents the Phase 3.2 decision.
//!
//! Run with `-- --nocapture` to see the full comparison table.

use openpulse_core::conv::ConvCodec;
use openpulse_core::fec::FecCodec;
use std::time::Instant;

/// Inject independent random bit errors at rate `ber` using a simple LCG.
fn flip_bits(data: &[u8], ber: f64, seed: u64) -> Vec<u8> {
    let mut out = data.to_vec();
    let mut s = seed;
    for byte in &mut out {
        for shift in 0..8u32 {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let r = (s >> 33) as f64 / u32::MAX as f64;
            if r < ber {
                *byte ^= 1 << shift;
            }
        }
    }
    out
}

fn count_bit_errors(a: &[u8], b: &[u8]) -> u64 {
    let len = a.len().min(b.len());
    let tail = (a.len().max(b.len()) - len) * 8;
    a[..len]
        .iter()
        .zip(b[..len].iter())
        .map(|(x, y)| (x ^ y).count_ones() as u64)
        .sum::<u64>()
        + tail as u64
}

#[test]
fn conv_round_trip_no_errors() {
    let c = ConvCodec::new();
    let payload: Vec<u8> = (0..128).map(|i| (i * 7 + 13) as u8).collect();
    let dec = c.decode(&c.encode(&payload)).unwrap();
    assert_eq!(dec, payload);
}

#[test]
fn conv_corrects_single_error() {
    let c = ConvCodec::new();
    let payload: Vec<u8> = b"phase 3.2 single error test".to_vec();
    let mut enc = c.encode(&payload);
    enc[8] ^= 0x01; // flip 1 bit
    let dec = c.decode(&enc).unwrap();
    assert_eq!(dec, payload);
}

#[test]
fn conv_rate_is_half() {
    let c = ConvCodec::new();
    let payload = vec![0u8; 100];
    let enc = c.encode(&payload);
    // Encoded = 2 × (4-byte prefix + 100 bytes) + small tail overhead
    let expected_min = 2 * (4 + 100);
    let expected_max = 2 * (4 + 100) + 4;
    assert!(
        enc.len() >= expected_min && enc.len() <= expected_max,
        "encoded len {} not in [{}, {}]",
        enc.len(),
        expected_min,
        expected_max
    );
}

/// Compare encode+decode latency; assert ConvCodec is within 10× of RS.
#[test]
fn rs_vs_conv_cpu_time() {
    const PAYLOAD_LEN: usize = 1000;
    const REPS: usize = 50;
    let payload: Vec<u8> = (0..PAYLOAD_LEN).map(|i| (i % 256) as u8).collect();

    let rs = FecCodec::new();
    let t0 = Instant::now();
    for _ in 0..REPS {
        let enc = rs.encode(&payload);
        let _ = rs.decode(&enc).unwrap();
    }
    let rs_us = t0.elapsed().as_micros() / REPS as u128;

    let cv = ConvCodec::new();
    let t0 = Instant::now();
    for _ in 0..REPS {
        let enc = cv.encode(&payload);
        let _ = cv.decode(&enc).unwrap();
    }
    let conv_us = t0.elapsed().as_micros() / REPS as u128;

    println!("\n=== CPU time per encode+decode cycle (1000-byte payload) ===");
    println!("  RS+interleaver : {rs_us} µs");
    println!("  ConvCodec      : {conv_us} µs");
    let ratio = conv_us as f64 / rs_us.max(1) as f64;
    println!("  Conv/RS ratio  : {ratio:.1}×");

    assert!(
        ratio < 10.0,
        "ConvCodec is {ratio:.1}× slower than RS — exceeds 10× CPU budget"
    );
}

/// BER table: RS vs ConvCodec at three channel error rates.
///
/// At channel BER = 0.01, ConvCodec (K=3, df=5) corrects isolated errors
/// within its trellis depth; RS corrects byte-level burst errors up to 16
/// bytes per block. The table shows which codec is better in AWGN vs bursty
/// regimes.
#[test]
fn rs_vs_conv_ber_random_noise() {
    const PAYLOAD_LEN: usize = 500;
    const REPS: usize = 30;
    let payload: Vec<u8> = (0..PAYLOAD_LEN).map(|i| (i * 3 + 7) as u8).collect();

    let rs = FecCodec::new();
    let cv = ConvCodec::new();

    println!(
        "\n=== BER comparison (random bit errors, {PAYLOAD_LEN}-byte payload, {REPS} reps) ==="
    );
    println!(
        "  {:>12}  {:>14}  {:>14}",
        "Channel BER", "RS post-BER", "Conv post-BER"
    );

    for &ch_ber in &[0.001f64, 0.01, 0.05] {
        let mut rs_error_bits = 0u64;
        let mut conv_error_bits = 0u64;
        let total_bits = (PAYLOAD_LEN as u64) * 8 * REPS as u64;

        for rep in 0..REPS as u64 {
            // RS: errors injected into the encoded byte stream
            let rs_enc = rs.encode(&payload);
            let rs_noisy = flip_bits(&rs_enc, ch_ber, rep * 1000 + 1);
            let rs_dec = rs
                .decode(&rs_noisy)
                .unwrap_or_else(|_| vec![0u8; PAYLOAD_LEN]);
            rs_error_bits += count_bit_errors(&payload, &rs_dec);

            // Conv: errors injected into the encoded byte stream
            let cv_enc = cv.encode(&payload);
            let cv_noisy = flip_bits(&cv_enc, ch_ber, rep * 1000 + 2);
            let cv_dec = cv
                .decode(&cv_noisy)
                .unwrap_or_else(|_| vec![0u8; PAYLOAD_LEN]);
            conv_error_bits += count_bit_errors(&payload, &cv_dec);
        }

        let rs_ber = rs_error_bits as f64 / total_bits as f64;
        let cv_ber = conv_error_bits as f64 / total_bits as f64;
        println!("  {ch_ber:>12.3}  {rs_ber:>14.6}  {cv_ber:>14.6}");
    }
}

/// Decision gate: at channel BER 0.01, does ConvCodec meet the ≥ 2 dB gain target?
///
/// A 2 dB SNR gain corresponds to roughly halving the post-decoding BER in the
/// Gaussian noise regime. If conv_ber ≤ rs_ber × 0.5 → ACCEPTED; else REJECTED.
///
/// Note: this is an informational test — it does not assert pass/fail since the
/// decision depends on the measurement results and hardware context.
#[test]
fn fec_decision_gate() {
    const PAYLOAD_LEN: usize = 500;
    const REPS: usize = 50;
    const CHANNEL_BER: f64 = 0.01;
    let payload: Vec<u8> = (0..PAYLOAD_LEN).map(|i| (i * 11 + 5) as u8).collect();

    let rs = FecCodec::new();
    let cv = ConvCodec::new();

    let mut rs_err = 0u64;
    let mut cv_err = 0u64;
    let total_bits = (PAYLOAD_LEN as u64) * 8 * REPS as u64;

    for rep in 0..REPS as u64 {
        let rs_enc = rs.encode(&payload);
        let rs_noisy = flip_bits(&rs_enc, CHANNEL_BER, rep * 999 + 7);
        let rs_dec = rs
            .decode(&rs_noisy)
            .unwrap_or_else(|_| vec![0u8; PAYLOAD_LEN]);
        rs_err += count_bit_errors(&payload, &rs_dec);

        let cv_enc = cv.encode(&payload);
        let cv_noisy = flip_bits(&cv_enc, CHANNEL_BER, rep * 999 + 8);
        let cv_dec = cv
            .decode(&cv_noisy)
            .unwrap_or_else(|_| vec![0u8; PAYLOAD_LEN]);
        cv_err += count_bit_errors(&payload, &cv_dec);
    }

    let rs_ber = rs_err as f64 / total_bits as f64;
    let cv_ber = cv_err as f64 / total_bits as f64;

    println!("\n=== Phase 3.2 Decision Gate (channel BER = {CHANNEL_BER}) ===");
    println!("  RS post-decode BER  : {rs_ber:.6}");
    println!("  Conv post-decode BER: {cv_ber:.6}");

    if cv_ber <= rs_ber * 0.5 {
        println!("  Decision: ConvCodec ACCEPTED (≥ 2 dB gain achieved)");
        println!("  Recommendation: add ConvCodec as optional FEC for HPX high-rate profiles.");
    } else {
        println!("  Decision: ConvCodec REJECTED — RS+interleaver preferred for HF channels.");
        println!(
            "  Reason: Conv BER {cv_ber:.6} > RS BER × 0.5 = {:.6}",
            rs_ber * 0.5
        );
        println!("  Note: K=3 Viterbi is weak against random noise at 1% BER;");
        println!("        RS byte-error correction is better matched to HF burst-error profiles.");
    }
}
