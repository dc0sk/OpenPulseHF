//! JS8 field packers (JS8Call `varicode.cpp`): standard callsign → 28-bit, Maidenhead grid → 15-bit.
//!
//! These are the fields a Heartbeat frame carries (callsign + grid). The port is validated against
//! ground-truth values produced by the **verbatim upstream `Varicode::packCallsign`/`packGrid`
//! compiled against real Qt** (see the `tests` module). Group/compound callsigns (`@OPULSE`, `/P`
//! beyond the strip, hashed calls) and directed commands land with the message-grammar unit.

/// Callsign/grid alphabet (JS8Call `varicode.cpp` `alphanumeric`): index 0–9 digits, 10–35 `A`–`Z`,
/// 36 space, 37 `/`, 38 `@`.
const ALPHANUMERIC: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ /@";

/// Value `packGrid` returns for a grid shorter than 4 chars (`(1<<15)-1`).
pub const GRID_INVALID: u16 = (1 << 15) - 1;

fn alnum_index(c: u8) -> i64 {
    ALPHANUMERIC
        .iter()
        .position(|&x| x == c)
        .map_or(-1, |i| i as i64)
}

fn is_digit(c: u8) -> bool {
    c.is_ascii_digit()
}
fn is_alnum(c: u8) -> bool {
    c.is_ascii_digit() || c.is_ascii_uppercase()
}
fn is_alpha_or_space(c: u8) -> bool {
    c == b' ' || c.is_ascii_uppercase()
}

/// The JS8 `pack_callsign_pattern` per-position character classes, over a 6-char window:
/// `([0-9A-Z ])([0-9A-Z])([0-9])([A-Z ])([A-Z ])([A-Z ])`.
fn window_matches(w: &[u8]) -> bool {
    (is_alnum(w[0]) || w[0] == b' ')
        && is_alnum(w[1])
        && is_digit(w[2])
        && is_alpha_or_space(w[3])
        && is_alpha_or_space(w[4])
        && is_alpha_or_space(w[5])
}

/// Leftmost 6-char window of `s` satisfying [`window_matches`] (the upstream regex's `captured(0)`).
fn first_match(s: &[u8]) -> Option<[u8; 6]> {
    s.windows(6)
        .find(|w| window_matches(w))
        .map(|w| [w[0], w[1], w[2], w[3], w[4], w[5]])
}

/// Pack a standard callsign into its 28-bit integer (JS8Call `packCallsign`). Returns `0` for a
/// callsign that doesn't fit the standard grammar (upstream's `return packed` default). Group and
/// hashed callsigns (the `basecalls` map) are not handled here.
pub fn pack_callsign(value: &str) -> u32 {
    let mut callsign = value.trim().to_ascii_uppercase();
    if let Some(stripped) = callsign.strip_suffix("/P") {
        callsign = stripped.to_string();
    }
    // Regional workarounds (verbatim from upstream).
    if let Some(rest) = callsign.strip_prefix("3DA0") {
        callsign = format!("3D0{rest}");
    }
    if let Some(rest) = callsign.strip_prefix("3X") {
        if rest
            .as_bytes()
            .first()
            .is_some_and(|c| c.is_ascii_uppercase())
        {
            callsign = format!("Q{rest}");
        }
    }

    let n = callsign.len();
    if !(2..=6).contains(&n) {
        return 0;
    }

    // Space-padding permutations tried in upstream order; the last that matches wins.
    let mut permutations: Vec<String> = vec![callsign.clone()];
    match n {
        2 => permutations.push(format!(" {callsign}   ")),
        3 => {
            permutations.push(format!(" {callsign}  "));
            permutations.push(format!("{callsign}   "));
        }
        4 => {
            permutations.push(format!(" {callsign} "));
            permutations.push(format!("{callsign}  "));
        }
        5 => {
            permutations.push(format!(" {callsign}"));
            permutations.push(format!("{callsign} "));
        }
        _ => {}
    }

    let mut matched: Option<[u8; 6]> = None;
    for p in &permutations {
        if let Some(m) = first_match(p.as_bytes()) {
            matched = Some(m); // last match wins
        }
    }
    let m = match matched {
        Some(m) => m,
        None => return 0,
    };

    let mut packed = alnum_index(m[0]);
    packed = 36 * packed + alnum_index(m[1]);
    packed = 10 * packed + alnum_index(m[2]);
    packed = 27 * packed + alnum_index(m[3]) - 10;
    packed = 27 * packed + alnum_index(m[4]) - 10;
    packed = 27 * packed + alnum_index(m[5]) - 10;
    packed as u32
}

/// Convert a Maidenhead grid to (longitude, latitude) degrees (JS8Call `grid2deg`; a 4-char grid is
/// extended with `"mm"`).
fn grid2deg(grid4: &str) -> (f64, f64) {
    let g: Vec<u8> = {
        let up = grid4[..4.min(grid4.len())].to_ascii_uppercase();
        let mut v = up.into_bytes();
        v.push(b'm');
        v.push(b'm');
        v
    };
    let nlong = 180 - 20 * (g[0] as i64 - b'A' as i64);
    let n20d = 2 * (g[2] as i64 - b'0' as i64);
    let xminlong = 5.0 * ((g[4] as i64 - b'a' as i64) as f64 + 0.5);
    let dlong = nlong as f64 - n20d as f64 - xminlong / 60.0;
    let nlat = -90 + 10 * (g[1] as i64 - b'A' as i64) + (g[3] as i64 - b'0' as i64);
    let xminlat = 2.5 * ((g[5] as i64 - b'a' as i64) as f64 + 0.5);
    let dlat = nlat as f64 + xminlat / 60.0;
    (dlong, dlat)
}

/// Pack a Maidenhead grid into its 15-bit integer (JS8Call `packGrid`). Grids shorter than 4 chars
/// return [`GRID_INVALID`].
pub fn pack_grid(value: &str) -> u16 {
    let grid = value.trim();
    if grid.chars().count() < 4 {
        return GRID_INVALID;
    }
    let (dlong, dlat) = grid2deg(&grid.chars().take(4).collect::<String>());
    let ilong = dlong as i64; // truncate toward zero (C++ float→int)
    let ilat = (dlat + 90.0) as i64;
    (((ilong + 180) / 2) * 180 + ilat) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (callsign, packed) from verbatim upstream `Varicode::packCallsign` on real Qt.
    const CALL_VECTORS: &[(&str, u32)] = &[
        ("KN4CRD", 146_325_342),
        ("W1AW", 261_410_543),
        ("DC0SK", 94_491_818),
        ("G0ABC", 258_240_989),
        ("VK2XYZ", 223_655_686),
        ("JA1ABC", 136_619_732),
        ("N0P", 259_630_433),
        ("EA3", 101_249_351),
        ("2E0AAA", 16_927_380),
        ("3DA0XX", 23_833_844), // Swaziland workaround → 3D0XX
        ("AB1CDE", 73_045_156),
    ];

    /// (grid, packed) from verbatim upstream `Varicode::packGrid` on real Qt.
    const GRID_VECTORS: &[(&str, u16)] = &[
        ("EM73", 23_883),
        ("FN20", 22_990),
        ("JO65", 15_085),
        ("IO91", 16_341),
        ("QF22", 3_112),
        ("RE78", 408),
        ("AA00", 32_220),
        ("JN58", 15_258),
    ];

    #[test]
    fn pack_callsign_matches_upstream() {
        for (call, want) in CALL_VECTORS {
            assert_eq!(pack_callsign(call), *want, "callsign {call}");
        }
    }

    #[test]
    fn callsign_is_case_and_whitespace_insensitive() {
        assert_eq!(pack_callsign("kn4crd"), pack_callsign("KN4CRD"));
        assert_eq!(pack_callsign("  W1AW "), pack_callsign("W1AW"));
    }

    #[test]
    fn unpackable_callsign_is_zero() {
        assert_eq!(pack_callsign(""), 0);
        assert_eq!(pack_callsign("X"), 0); // too short
        assert_eq!(pack_callsign("TOOLONG7"), 0); // too long
    }

    #[test]
    fn pack_grid_matches_upstream() {
        for (grid, want) in GRID_VECTORS {
            assert_eq!(pack_grid(grid), *want, "grid {grid}");
        }
    }

    #[test]
    fn short_grid_is_invalid() {
        assert_eq!(pack_grid("EM"), GRID_INVALID);
        assert_eq!(pack_grid(""), GRID_INVALID);
    }
}
