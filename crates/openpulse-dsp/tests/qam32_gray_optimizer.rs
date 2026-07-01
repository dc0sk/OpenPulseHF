//! Derives a 2D-Gray-optimized cross-32QAM label→point table by minimizing the total Hamming
//! distance between Euclidean-nearest-neighbour constellation points (the quantity that drives
//! bit-error/soft-LLR quality). The current mapping (`QAM32_SPATIAL[gray5_to_natural(label)]`) is a
//! 1D-Gray over a 2D raster and is far from optimal. Ignored (a derivation tool); run:
//!
//! ```text
//! cargo test -p openpulse-dsp --test qam32_gray_optimizer -- --ignored --nocapture
//! ```

/// The 32 cross-32QAM points (6×6 minus the four corners), in raster order.
const POINTS: [(i8, i8); 32] = [
    (-3, 5),
    (-1, 5),
    (1, 5),
    (3, 5),
    (-5, 3),
    (-3, 3),
    (-1, 3),
    (1, 3),
    (3, 3),
    (5, 3),
    (-5, 1),
    (-3, 1),
    (-1, 1),
    (1, 1),
    (3, 1),
    (5, 1),
    (-5, -1),
    (-3, -1),
    (-1, -1),
    (1, -1),
    (3, -1),
    (5, -1),
    (-5, -3),
    (-3, -3),
    (-1, -3),
    (1, -3),
    (3, -3),
    (5, -3),
    (-3, -5),
    (-1, -5),
    (1, -5),
    (3, -5),
];

fn natural5_to_gray(n: u8) -> u8 {
    (n ^ (n >> 1)) & 0x1f
}

fn hamming(a: u8, b: u8) -> u32 {
    (a ^ b).count_ones()
}

/// Nearest-neighbour pairs (squared distance 4) weighted 1.0, diagonal (dist² 8) weighted 0.5.
#[allow(clippy::needless_range_loop)] // triangular i/j+1 scan over POINTS — index-based by nature
fn neighbour_pairs() -> Vec<(usize, usize, f32)> {
    let mut v = Vec::new();
    for i in 0..32 {
        for j in (i + 1)..32 {
            let (ai, aq) = POINTS[i];
            let (bi, bq) = POINTS[j];
            let d2 = (ai as i32 - bi as i32).pow(2) + (aq as i32 - bq as i32).pow(2);
            if d2 == 4 {
                v.push((i, j, 1.0));
            } else if d2 == 8 {
                v.push((i, j, 0.5));
            }
        }
    }
    v
}

/// Cost = Σ weight · Hamming(label[i], label[j]) over neighbour pairs. `label[point] = 5-bit label`.
fn cost(label: &[u8; 32], pairs: &[(usize, usize, f32)]) -> f32 {
    pairs
        .iter()
        .map(|&(i, j, w)| w * hamming(label[i], label[j]) as f32)
        .sum()
}

// Tiny deterministic xorshift RNG (no external dep, reproducible).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn f(&mut self) -> f32 {
        (self.next() >> 11) as f32 / (1u64 << 53) as f32
    }
}

#[test]
#[ignore = "constellation-mapping derivation tool; run with --ignored --nocapture"]
fn derive_qam32_gray_mapping() {
    let pairs = neighbour_pairs();

    // Baseline = the current mapping: point at raster index r carries label natural5_to_gray(r).
    let baseline: [u8; 32] = std::array::from_fn(|r| natural5_to_gray(r as u8));
    let base_cost = cost(&baseline, &pairs);

    // Simulated annealing over label permutations (labels 0..31 assigned to the 32 points).
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut best = baseline;
    let mut best_cost = base_cost;

    for _restart in 0..250 {
        // Random start permutation of labels 0..31.
        let mut label: [u8; 32] = std::array::from_fn(|i| i as u8);
        for i in (1..32).rev() {
            let j = rng.below(i + 1);
            label.swap(i, j);
        }
        let mut cur = cost(&label, &pairs);
        let mut t = 6.0f32;
        for _ in 0..150_000 {
            let a = rng.below(32);
            let b = rng.below(32);
            if a == b {
                continue;
            }
            label.swap(a, b);
            let c = cost(&label, &pairs);
            let d = c - cur;
            if d <= 0.0 || rng.f() < (-d / t).exp() {
                cur = c;
            } else {
                label.swap(a, b); // reject
            }
            t *= 0.99992;
        }
        if cur < best_cost {
            best_cost = cur;
            best = label;
        }
    }

    // Invert: QAM32_BY_LABEL[label] = point.
    let mut by_label = [(0i8, 0i8); 32];
    for (p, &l) in best.iter().enumerate() {
        by_label[l as usize] = POINTS[p];
    }

    println!("\n=== cross-32QAM Gray mapping optimization ===");
    println!("baseline neighbour-Hamming cost: {base_cost:.1}");
    println!("optimized neighbour-Hamming cost: {best_cost:.1}  (lower is better)");
    println!(
        "avg bits/nearest-neighbour: baseline={:.3} optimized={:.3}",
        base_cost / pairs.iter().map(|&(_, _, w)| w).sum::<f32>(),
        best_cost / pairs.iter().map(|&(_, _, w)| w).sum::<f32>()
    );
    println!("\npub const QAM32_BY_LABEL: [(i8, i8); 32] = [");
    for (l, (i, q)) in by_label.iter().enumerate() {
        println!("    ({i}, {q}), // {l:05b}");
    }
    println!("];");

    // Sanity: the optimized assignment is a bijection over all 32 points.
    let mut seen = [false; 32];
    for &(i, q) in by_label.iter() {
        let idx = POINTS
            .iter()
            .position(|&p| p == (i, q))
            .expect("point in set");
        assert!(!seen[idx], "duplicate point in optimized table");
        seen[idx] = true;
    }
    assert!(best_cost < base_cost, "optimizer must beat the baseline");
}
