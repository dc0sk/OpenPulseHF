//! JS8 word-dictionary free-text decode (JS8Call `jsc.cpp` `JSC::decompress`).
//!
//! JS8's `packDataMessage` chooses, per frame, whichever of Huffman ([`crate::varicode`]) or this
//! `(s, c)`-dense codebook coder packs more characters. The codebook is the 262 144-entry
//! frequency-sorted word/substring table ported verbatim from GPL-3.0 JS8Call (`jsc_map.cpp`),
//! embedded here as a zlib-compressed length-prefixed blob and expanded once on first use.
//!
//! Decoding a data frame's content bits (the same bits [`crate::varicode::unpack_data_message`] feeds
//! Huffman) reproduces `JSC::decompress`: read 4-bit groups (each low group `< s` optionally followed
//! by a separator bit), then fold each run of high groups into a base-`c` index, add the terminating
//! low group plus the run-length's cumulative `base`, and emit `map[index]`. Validated against the
//! verbatim upstream `Varicode::unpackDataMessage` (JSC branch) compiled against real Qt5.

use std::sync::OnceLock;

use flate2::read::ZlibDecoder;
use std::io::Read;

/// Dense-code parameters (JS8Call `jsc.cpp`): 4-bit groups, `s` low symbols, `c = 2^4 - s` high.
const S: u32 = 7;
const C: u32 = 16 - S;
/// Codebook size (`JSC::size`).
const SIZE: u32 = 262_144;

/// The expanded codebook: a byte pool plus `(offset, len)` per entry, resolved once.
struct CodeBook {
    pool: Vec<u8>,
    entries: Vec<(u32, u8)>,
}

impl CodeBook {
    fn word(&self, index: usize) -> &[u8] {
        let (off, len) = self.entries[index];
        &self.pool[off as usize..off as usize + len as usize]
    }
}

/// zlib-compressed `[count:u32 LE][ (len:u8)(latin1 bytes) × count ]` (generated from `jsc_map.cpp`).
static CODEBOOK_Z: &[u8] = include_bytes!("../data/jsc_codebook.bin.z");

fn codebook() -> &'static CodeBook {
    static CB: OnceLock<CodeBook> = OnceLock::new();
    CB.get_or_init(|| {
        let mut pool = Vec::new();
        ZlibDecoder::new(CODEBOOK_Z)
            .read_to_end(&mut pool)
            .expect("embedded JSC codebook decompresses");
        let count = u32::from_le_bytes([pool[0], pool[1], pool[2], pool[3]]) as usize;
        let mut entries = Vec::with_capacity(count);
        let mut off = 4usize;
        for _ in 0..count {
            let len = pool[off];
            entries.push((off as u32 + 1, len));
            off += 1 + len as usize;
        }
        CodeBook { pool, entries }
    })
}

/// Cumulative `base[k]` (JS8Call `jsc.cpp`): `base[0]=0`, `base[k]=base[k-1]+s·c^(k-1)`.
fn base(k: usize) -> u32 {
    let mut b = 0u32;
    let mut pow = 1u32;
    for _ in 0..k {
        b += S * pow;
        pow *= C;
    }
    b
}

/// Interpret `bits` as a big-endian integer (JS8Call `bitsToInt`).
fn bits_to_int(bits: &[bool]) -> u32 {
    bits.iter().fold(0u32, |acc, &b| (acc << 1) | b as u32)
}

/// Decode a data frame's content bits with the JSC codebook (JS8Call `JSC::decompress`). Returns the
/// recovered free text; a malformed stream stops early like upstream.
pub fn jsc_decompress(bits: &[bool]) -> String {
    // Phase 1: split into 4-bit groups; a low group (`< s`) may be followed by a separator bit.
    let mut groups: Vec<u32> = Vec::new();
    let mut separators: Vec<usize> = Vec::new();
    let mut i = 0usize;
    while i < bits.len() {
        if i + 4 > bits.len() {
            break;
        }
        let g = bits_to_int(&bits[i..i + 4]);
        groups.push(g);
        i += 4;
        if g < S {
            if i < bits.len() && bits[i] {
                separators.push(groups.len() - 1);
            }
            i += 1;
        }
    }

    // Phase 2: fold high-group runs into an index, emit the codebook word, honour separators.
    let cb = codebook();
    let mut out: Vec<u8> = Vec::new();
    let mut sep = 0usize;
    let mut start = 0usize;
    while start < groups.len() {
        let mut k = 0usize;
        let mut j = 0u32;
        while start + k < groups.len() && groups[start + k] >= S {
            j = j * C + (groups[start + k] - S);
            k += 1;
        }
        if j >= SIZE || start + k >= groups.len() {
            break;
        }
        j = j * S + groups[start + k] + base(k);
        if j >= SIZE {
            break;
        }
        out.extend_from_slice(cb.word(j as usize));
        if sep < separators.len() && separators[sep] == start + k {
            out.push(b' ');
            sep += 1;
        }
        start += k + 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::varicode::unpack_data_message;

    fn hex9(s: &str) -> [u8; 9] {
        let mut p = [0u8; 9];
        for (i, b) in p.iter_mut().enumerate() {
            *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap();
        }
        p
    }

    #[test]
    fn codebook_expands_to_the_expected_size() {
        let cb = codebook();
        assert_eq!(cb.entries.len(), 262_144);
        assert_eq!(cb.word(0), b"E");
        assert_eq!(cb.word(1), b"T");
    }

    #[test]
    fn diverse_data_frames_match_upstream() {
        // A spread of texts hitting single chars, multi-word runs, and high dictionary indices
        // (e.g. ABCDEFGHIJK at index ~220k). (payload9, decoded) from verbatim upstream; `unpack_data_message`
        // routes comp=0 to Huffman and comp=1 to JSC.
        for (hex, want) in [
            ("de2e16f7287fffffff", "HELLO WORLD"),
            ("c4e00e7797e5daffff", "THE QUICK BROWN "),
            ("de5f10ffffffffffff", "ARGENTINA"),
            ("f3bc8fffffffffffff", "QUICK"),
            ("dff0cfffffffffffff", "NOVEMBER"),
            ("ff13dcc7ffffffffff", "WEATHER REPORT"),
            ("eec79618a13fffffff", "73 AND GL"),
            ("de3831654362ae80ff", "MY GRID IS EM73"),
            ("de2ee8ef9b5027ffff", "ZULU XRAY"),
            ("88890bea13c3afc7d1", "123456789"),
            ("f74313607fffffffff", "GOOD MORNING"),
            ("eb130e47ffffffffff", "QRP DX"),
            ("f7d0ffffffffffffff", "INFORMATION"),
            ("e2b6a44f977487ffff", "ZYXWVU"),
            ("e3a40fffffffffffff", "INTERNATIONAL"),
        ] {
            assert_eq!(
                unpack_data_message(&hex9(hex)).as_deref(),
                Some(want),
                "decode of {hex}"
            );
        }
    }

    #[test]
    fn jsc_data_frames_match_upstream() {
        // (payload9, decoded) from verbatim upstream packDataMessage/unpackDataMessage (comp=1 cases).
        for (hex, want) in [
            ("e618e081581fffffff", "OPHF1"),
            ("de2e14ffffffffffff", "HELLO"),
            ("e748ffffffffffffff", "TEST"),
            ("df8afc47ffffffffff", "CQ CQ"),
            ("eec736a7ffffffffff", "73 SK"),
            ("eb622448a6491fffff", "ABCDEFGHIJK"),
            ("ed8e4c13ffffffffff", "3D4"),
            ("de48ffffffffffffff", "AGN?"),
            ("dc55e47fffffffffff", "R-12"),
            ("c4e00e7791ffffffff", "THE QUICK"),
            ("c96085599d0fffffff", "A1B2C"),
        ] {
            assert_eq!(
                unpack_data_message(&hex9(hex)),
                Some(want.to_string()),
                "JSC decode of {hex}"
            );
        }
    }
}
