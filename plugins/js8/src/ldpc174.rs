//! LDPC(174,87) generator + parity tables and systematic encoder.
//!
//! Ported verbatim from JS8Call/js8call (GPL-3.0) `lib/ft8/ldpc_174_87_params.f90`
//! (generator `g`, `colorder`) and `lib/ft8/bpdecode174.f90` (parity incidence `Nm`, weights
//! `nrw`). This is the FT8 v1 code JS8 froze on. Belief-propagation *decode* is Phase B; this
//! unit is the encoder + a parity syndrome used to prove the port (a valid codeword has zero
//! syndrome).

/// Codeword length.
pub const N: usize = 174;
/// Information-bit length (75 message + 12 CRC).
pub const K: usize = 87;
/// Parity-check count (`N - K`).
pub const M: usize = N - K;

/// Parity generator matrix rows (each 22 hex chars = 88 bits; the low 87 are the row over GF(2)).
const G_HEX: [&str; M] = [
    "23bba830e23b6b6f50982e",
    "1f8e55da218c5df3309052",
    "ca7b3217cd92bd59a5ae20",
    "56f78313537d0f4382964e",
    "29c29dba9c545e267762fe",
    "6be396b5e2e819e373340c",
    "293548a138858328af4210",
    "cb6c6afcdc28bb3f7c6e86",
    "3f2a86f5c5bd225c961150",
    "849dd2d63673481860f62c",
    "56cdaec6e7ae14b43feeee",
    "04ef5cfa3766ba778f45a4",
    "c525ae4bd4f627320a3974",
    "fe37802941d66dde02b99c",
    "41fd9520b2e4abeb2f989c",
    "40907b01280f03c0323946",
    "7fb36c24085a34d8c1dbc4",
    "40fc3e44bb7d2bb2756e44",
    "d38ab0a1d2e52a8ec3bc76",
    "3d0f929ef3949bd84d4734",
    "45d3814f504064f80549ae",
    "f14dbf263825d0bd04b05e",
    "f08a91fb2e1f78290619a8",
    "7a8dec79a51e8ac5388022",
    "ca4186dd44c3121565cf5c",
    "db714f8f64e8ac7af1a76e",
    "8d0274de71e7c1a8055eb0",
    "51f81573dd4049b082de14",
    "d037db825175d851f3af00",
    "d8f937f31822e57c562370",
    "1bf1490607c54032660ede",
    "1616d78018d0b4745ca0f2",
    "a9fa8e50bcb032c85e3304",
    "83f640f1a48a8ebc0443ea",
    "eca9afa0f6b01d92305edc",
    "3776af54ccfbae916afde6",
    "6abb212d9739dfc02580f2",
    "05209a0abb530b9e7e34b0",
    "612f63acc025b6ab476f7c",
    "0af7723161ec223080be86",
    "a8fc906976c35669e79ce0",
    "45b7ab6242b77474d9f11a",
    "b274db8abd3c6f396ea356",
    "9059dfa2bb20ef7ef73ad4",
    "3d188ea477f6fa41317a4e",
    "8d9071b7e7a6a2eed6965e",
    "a377253773ea678367c3f6",
    "ecbd7c73b9cd34c3720c8a",
    "b6537f417e61d1a7085336",
    "6c280d2a0523d9c4bc5946",
    "d36d662a69ae24b74dcbd8",
    "d747bfc5fd65ef70fbd9bc",
    "a9fa2eefa6f8796a355772",
    "cc9da55fe046d0cb3a770c",
    "f6ad4824b87c80ebfce466",
    "cc6de59755420925f90ed2",
    "164cc861bdd803c547f2ac",
    "c0fc3ec4fb7d2bb2756644",
    "0dbd816fba1543f721dc72",
    "a0c0033a52ab6299802fd2",
    "bf4f56e073271f6ab4bf80",
    "57da6d13cb96a7689b2790",
    "81cfc6f18c35b1e1f17114",
    "481a2a0df8a23583f82d6c",
    "1ac4672b549cd6dba79bcc",
    "c87af9a5d5206abca532a8",
    "97d4169cb33e7435718d90",
    "a6573f3dc8b16c9d19f746",
    "2c4142bf42b01e71076acc",
    "081c29a10d468ccdbcecb6",
    "5b0f7742bca86b8012609a",
    "012dee2198eba82b19a1da",
    "f1627701a2d692fd9449e6",
    "35ad3fb0faeb5f1b0c30dc",
    "b1ca4ea2e3d173bad4379c",
    "37d8e0af9258b9e8c5f9b2",
    "cd921fdf59e882683763f6",
    "6114e08483043fd3f38a8a",
    "2e547dd7a05f6597aac516",
    "95e45ecd0135aca9d6e6ae",
    "b33ec97be83ce413f9acc8",
    "c8b5dffc335095dcdcaf2a",
    "3dd01a59d86310743ec752",
    "14cd0f642fc0c5fe3a65ca",
    "3a0a1dfd7eee29c2e827e0",
    "8abdb889efbe39a510a118",
    "3f231f212055371cf3e2a2",
];

/// Column reordering applied after systematic assembly (0-based, from source).
const COLORDER: [usize; N] = [
    0, 1, 2, 3, 30, 4, 5, 6, 7, 8, 9, 10, 11, 32, 12, 40, 13, 14, 15, 16, 17, 18, 37, 45, 29, 19,
    20, 21, 41, 22, 42, 31, 33, 34, 44, 35, 47, 51, 50, 43, 36, 52, 63, 46, 25, 55, 27, 24, 23, 53,
    39, 49, 59, 38, 48, 61, 60, 57, 28, 62, 56, 58, 65, 66, 26, 70, 64, 69, 68, 67, 74, 71, 54, 76,
    72, 75, 78, 77, 80, 79, 73, 83, 84, 81, 82, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97,
    98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116,
    117, 118, 119, 120, 121, 122, 123, 124, 125, 126, 127, 128, 129, 130, 131, 132, 133, 134, 135,
    136, 137, 138, 139, 140, 141, 142, 143, 144, 145, 146, 147, 148, 149, 150, 151, 152, 153, 154,
    155, 156, 157, 158, 159, 160, 161, 162, 163, 164, 165, 166, 167, 168, 169, 170, 171, 172, 173,
];

/// Per-check variable-node incidence (1-based as in source; 0 = unused slot). `nrw` entries used.
const NM: [[u8; 7]; M] = [
    [1, 30, 60, 89, 118, 147, 0],
    [2, 31, 61, 90, 119, 147, 0],
    [3, 32, 62, 91, 120, 148, 0],
    [4, 33, 63, 92, 121, 149, 0],
    [2, 34, 64, 93, 122, 150, 0],
    [5, 33, 65, 94, 123, 148, 0],
    [6, 34, 66, 95, 124, 151, 0],
    [7, 35, 67, 96, 120, 152, 0],
    [8, 36, 68, 97, 125, 153, 0],
    [9, 37, 69, 98, 126, 152, 0],
    [10, 38, 70, 99, 127, 154, 0],
    [11, 39, 71, 100, 126, 155, 0],
    [12, 40, 61, 101, 128, 145, 0],
    [10, 33, 60, 95, 128, 156, 0],
    [13, 41, 72, 97, 126, 157, 0],
    [13, 42, 73, 90, 129, 156, 0],
    [14, 39, 74, 99, 130, 158, 0],
    [15, 43, 75, 102, 131, 159, 0],
    [16, 43, 71, 103, 118, 160, 0],
    [17, 44, 76, 98, 130, 156, 0],
    [18, 45, 60, 96, 132, 161, 0],
    [19, 46, 73, 83, 133, 162, 0],
    [12, 38, 77, 102, 134, 163, 0],
    [19, 47, 78, 104, 135, 147, 0],
    [1, 32, 77, 105, 136, 164, 0],
    [20, 48, 73, 106, 123, 163, 0],
    [21, 41, 79, 107, 137, 165, 0],
    [22, 42, 66, 108, 138, 152, 0],
    [18, 42, 80, 109, 139, 154, 0],
    [23, 49, 81, 110, 135, 166, 0],
    [16, 50, 82, 91, 129, 158, 0],
    [3, 48, 63, 107, 124, 167, 0],
    [6, 51, 67, 111, 134, 155, 0],
    [24, 35, 77, 100, 122, 162, 0],
    [20, 45, 76, 112, 140, 157, 0],
    [21, 36, 64, 92, 130, 159, 0],
    [8, 52, 83, 111, 118, 166, 0],
    [21, 53, 84, 113, 138, 168, 0],
    [25, 51, 79, 89, 122, 158, 0],
    [22, 44, 75, 107, 133, 155, 172],
    [9, 54, 84, 90, 141, 169, 0],
    [22, 54, 85, 110, 136, 161, 0],
    [8, 37, 65, 102, 129, 170, 0],
    [19, 39, 85, 114, 139, 150, 0],
    [26, 55, 71, 93, 142, 167, 0],
    [27, 56, 65, 96, 133, 160, 174],
    [28, 31, 86, 100, 117, 171, 0],
    [28, 52, 70, 104, 132, 144, 0],
    [24, 57, 68, 95, 137, 142, 0],
    [7, 30, 72, 110, 143, 151, 0],
    [4, 51, 76, 115, 127, 168, 0],
    [16, 45, 87, 114, 125, 172, 0],
    [15, 30, 86, 115, 123, 150, 0],
    [23, 46, 64, 91, 144, 173, 0],
    [23, 35, 75, 113, 145, 153, 0],
    [14, 41, 87, 108, 117, 149, 170],
    [25, 40, 85, 94, 124, 159, 0],
    [25, 58, 69, 116, 143, 174, 0],
    [29, 43, 61, 116, 132, 162, 0],
    [15, 58, 88, 112, 121, 164, 0],
    [4, 59, 72, 114, 119, 163, 173],
    [27, 47, 86, 98, 134, 153, 0],
    [5, 44, 78, 109, 141, 0, 0],
    [10, 46, 69, 103, 136, 165, 0],
    [9, 50, 59, 93, 128, 164, 0],
    [14, 57, 58, 109, 120, 166, 0],
    [17, 55, 62, 116, 125, 154, 0],
    [3, 54, 70, 101, 140, 170, 0],
    [1, 36, 82, 108, 127, 174, 0],
    [5, 53, 81, 105, 140, 0, 0],
    [29, 53, 67, 99, 142, 173, 0],
    [18, 49, 74, 97, 115, 167, 0],
    [2, 57, 63, 103, 138, 157, 0],
    [26, 38, 79, 112, 135, 171, 0],
    [11, 52, 66, 88, 119, 148, 0],
    [20, 40, 68, 117, 141, 160, 0],
    [11, 48, 81, 89, 146, 169, 0],
    [29, 47, 80, 92, 146, 172, 0],
    [6, 32, 87, 104, 145, 169, 0],
    [27, 34, 74, 106, 131, 165, 0],
    [12, 56, 84, 88, 139, 0, 0],
    [13, 56, 62, 111, 146, 171, 0],
    [26, 37, 80, 105, 144, 151, 0],
    [17, 31, 82, 113, 121, 161, 0],
    [28, 49, 59, 94, 137, 0, 0],
    [7, 55, 83, 101, 131, 168, 0],
    [24, 50, 78, 106, 143, 149, 0],
];

/// Number of variable nodes per check (row weight of `NM`).
const NRW: [usize; M] = [
    6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6,
    6, 6, 6, 6, 6, 6, 6, 7, 6, 6, 6, 6, 6, 7, 6, 6, 6, 6, 6, 6, 6, 6, 6, 7, 6, 6, 6, 6, 7, 6, 5, 6,
    6, 6, 6, 6, 6, 5, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 5, 6, 6, 6, 5, 6, 6,
];

/// Hex-nibble value (`0` for non-hex, unreachable for our tables).
const fn hex_val(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    }
}

/// Generator bit at row `row` (0..M), column `col` (0..K) over GF(2). Column `c` lives in byte
/// `c/8` of the hex row, MSB-first within the byte (JS8Call `encode174` `btest(istr, 8-jj)`).
fn gen_bit(row: usize, col: usize) -> u8 {
    let s = G_HEX[row].as_bytes();
    let byte = (hex_val(s[(col / 8) * 2]) << 4) | hex_val(s[(col / 8) * 2 + 1]);
    (byte >> (7 - (col % 8))) & 1
}

/// Systematically encode an 87-bit `message` (each entry 0/1) into a 174-bit codeword.
///
/// `pchecks = G · message` over GF(2), then `[pchecks | message]` is column-reordered by `COLORDER`
/// (JS8Call `encode174`). Non-0/1 message entries are masked to their low bit.
pub fn encode174(message: &[u8; K]) -> [u8; N] {
    let mut itmp = [0u8; N];
    for (i, slot) in itmp.iter_mut().enumerate().take(M) {
        let mut acc = 0u8;
        for (j, &mj) in message.iter().enumerate() {
            acc ^= (mj & 1) & gen_bit(i, j);
        }
        *slot = acc & 1;
    }
    for (j, &mj) in message.iter().enumerate() {
        itmp[M + j] = mj & 1;
    }
    let mut cw = [0u8; N];
    for (i, &dst) in COLORDER.iter().enumerate() {
        cw[dst] = itmp[i];
    }
    cw
}

/// Parity syndrome of a 174-bit `codeword`: `H · codeword` over GF(2), one bit per check. A valid
/// codeword yields all zeros. (`NM` is 1-based, `nrw` entries used.)
pub fn parity_syndrome(codeword: &[u8; N]) -> [u8; M] {
    let mut synd = [0u8; M];
    for (i, s) in synd.iter_mut().enumerate() {
        let mut acc = 0u8;
        for &v in NM[i].iter().take(NRW[i]) {
            acc ^= codeword[(v as usize) - 1] & 1;
        }
        *s = acc & 1;
    }
    synd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_sizes() {
        assert_eq!(G_HEX.len(), M);
        assert_eq!(COLORDER.len(), N);
        assert_eq!(NM.len(), M);
        assert_eq!(NRW.len(), M);
        // Every colorder entry is a distinct destination in 0..N (a permutation).
        let mut seen = [false; N];
        for &d in &COLORDER {
            assert!(d < N && !seen[d], "colorder not a permutation at {d}");
            seen[d] = true;
        }
        // nrw matches the nonzero count of each NM row.
        for (i, row) in NM.iter().enumerate() {
            assert_eq!(row.iter().filter(|&&v| v != 0).count(), NRW[i], "row {i}");
        }
    }

    #[test]
    fn every_codeword_satisfies_all_parity_checks() {
        // The port is correct iff G and H are mutually consistent: H·(G·m) = 0 for all m. Sweep many
        // pseudo-random messages (deterministic LCG) — a single wrong generator/colorder/Nm entry
        // would break some check on some message.
        let mut state: u64 = 0x1234_5678_9abc_def1;
        for _ in 0..500 {
            let mut msg = [0u8; K];
            for m in msg.iter_mut() {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                *m = ((state >> 33) & 1) as u8;
            }
            let cw = encode174(&msg);
            assert_eq!(parity_syndrome(&cw), [0u8; M], "nonzero syndrome");
        }
    }

    #[test]
    fn all_zero_message_encodes_to_all_zero() {
        assert_eq!(encode174(&[0u8; K]), [0u8; N]);
    }

    #[test]
    fn message_bits_survive_into_the_codeword() {
        // The systematic message occupies itmp[M..N], reordered by COLORDER — recover and compare.
        let mut msg = [0u8; K];
        for (j, m) in msg.iter_mut().enumerate() {
            *m = (j % 3 == 0) as u8;
        }
        let cw = encode174(&msg);
        for (j, &want) in msg.iter().enumerate() {
            assert_eq!(cw[COLORDER[M + j]], want, "message bit {j}");
        }
    }
}
