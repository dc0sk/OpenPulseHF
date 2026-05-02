use std::f32::consts::PI;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::ModulationConfig;

use crate::modulate::{
    gray_map_8psk, preamble_symbols, samples_per_symbol, PREAMBLE_SYMS, TAIL_SYMS,
};
use crate::parse_baud_rate;

pub fn psk8_demodulate(samples: &[f32], config: &ModulationConfig) -> Result<Vec<u8>, ModemError> {
    let baud = parse_baud_rate(&config.mode)?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let n = samples_per_symbol(fs, baud)?;

    if samples.len() < n * (PREAMBLE_SYMS + 1) {
        return Err(ModemError::Demodulation("signal too short".to_string()));
    }

    let timing = find_timing_offset(samples, n, fc, fs);
    let syms = demodulate_symbols(samples, n, fc, fs, timing);
    if syms.len() <= PREAMBLE_SYMS + TAIL_SYMS {
        return Err(ModemError::Demodulation(
            "no data symbols after preamble".to_string(),
        ));
    }

    let data = &syms[PREAMBLE_SYMS..(syms.len() - TAIL_SYMS)];
    let bits = symbols_to_bits(data);
    Ok(bits_to_bytes(&bits))
}

fn find_timing_offset(samples: &[f32], n: usize, fc: f32, fs: f32) -> usize {
    let expected = preamble_symbols();
    let mut best_off = 0usize;
    let mut best_score = f32::NEG_INFINITY;

    for off in 0..n {
        if samples.len() <= off + n * PREAMBLE_SYMS {
            break;
        }
        let syms = demodulate_symbols(samples, n, fc, fs, off);
        if syms.len() < PREAMBLE_SYMS {
            continue;
        }
        let score: f32 = syms
            .iter()
            .zip(expected.iter())
            .take(PREAMBLE_SYMS)
            .map(|(&(i, q), &(ei, eq))| i * ei + q * eq)
            .sum();
        if score > best_score {
            best_score = score;
            best_off = off;
        }
    }

    best_off
}

fn demodulate_symbols(
    samples: &[f32],
    n: usize,
    fc: f32,
    fs: f32,
    offset: usize,
) -> Vec<(f32, f32)> {
    let two_pi = 2.0 * PI;
    let aligned = &samples[offset.min(samples.len())..];
    let n_syms = aligned.len() / n;
    let mut out = Vec::with_capacity(n_syms);

    for sym_idx in 0..n_syms {
        let start = sym_idx * n;
        let mut i_acc = 0.0f32;
        let mut q_acc = 0.0f32;
        let mut norm = 0.0f32;

        for i in 0..n {
            let g = (offset + start + i) as f32;
            let sample = aligned[start + i];
            let window = 0.5 * (1.0 - (two_pi * i as f32 / n as f32).cos());
            let t = g / fs;
            let c = (two_pi * fc * t).cos();
            let s = (two_pi * fc * t).sin();

            i_acc += sample * c * window * 2.0;
            q_acc += -sample * s * window * 2.0;
            norm += window * window;
        }

        if norm > 1e-9 {
            i_acc /= norm;
            q_acc /= norm;
        }

        out.push((i_acc, q_acc));
    }

    out
}

fn symbols_to_bits(symbols: &[(f32, f32)]) -> Vec<bool> {
    let mut bits = Vec::with_capacity(symbols.len() * 3);
    for &(i, q) in symbols {
        let (b0, b1, b2) = nearest_gray_triplet(i, q);
        bits.push(b0);
        bits.push(b1);
        bits.push(b2);
    }
    bits
}

type Candidate = ((f32, f32), (bool, bool, bool));

fn nearest_gray_triplet(i: f32, q: f32) -> (bool, bool, bool) {
    let candidates: [Candidate; 8] = [
        (gray_map_8psk(false, false, false), (false, false, false)),
        (gray_map_8psk(false, false, true), (false, false, true)),
        (gray_map_8psk(false, true, true), (false, true, true)),
        (gray_map_8psk(false, true, false), (false, true, false)),
        (gray_map_8psk(true, true, false), (true, true, false)),
        (gray_map_8psk(true, true, true), (true, true, true)),
        (gray_map_8psk(true, false, true), (true, false, true)),
        (gray_map_8psk(true, false, false), (true, false, false)),
    ];

    let mut best = (false, false, false);
    let mut best_dist = f32::INFINITY;
    for &((ci, cq), bits) in &candidates {
        let di = i - ci;
        let dq = q - cq;
        let dist = di * di + dq * dq;
        if dist < best_dist {
            best_dist = dist;
            best = bits;
        }
    }
    best
}

fn bits_to_bytes(bits: &[bool]) -> Vec<u8> {
    bits.chunks(8)
        .map(|chunk| {
            chunk
                .iter()
                .enumerate()
                .fold(0u8, |acc, (i, &b)| acc | ((b as u8) << i))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpulse_core::plugin::ModulationConfig;

    #[test]
    fn psk8_round_trip_500() {
        let cfg = ModulationConfig {
            mode: "8PSK500".to_string(),
            ..ModulationConfig::default()
        };
        let payload = b"OpenPulse 8PSK";
        let samples = crate::modulate::psk8_modulate(payload, &cfg).expect("modulate");
        let recovered = psk8_demodulate(&samples, &cfg).expect("demodulate");
        assert_eq!(&recovered[..payload.len()], payload);
    }
}
