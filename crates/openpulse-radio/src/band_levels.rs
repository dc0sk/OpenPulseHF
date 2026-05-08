//! IARU amateur band segment table and per-band TX attenuation helpers.

use std::collections::HashMap;

/// Map a frequency in Hz to its IARU amateur band name.
///
/// Returns the conventional band label (e.g. `"40m"`) or `"default"` when
/// the frequency falls outside all registered allocations.
pub fn freq_hz_to_band(hz: u64) -> &'static str {
    // IARU Region 1 / FCC Part 97 band edges (Hz).
    // Listed in ascending order; first match wins.
    const BANDS: &[(u64, u64, &str)] = &[
        (1_800_000, 2_000_000, "160m"),
        (3_500_000, 4_000_000, "80m"),
        (5_330_500, 5_403_500, "60m"),
        (7_000_000, 7_300_000, "40m"),
        (10_100_000, 10_150_000, "30m"),
        (14_000_000, 14_350_000, "20m"),
        (18_068_000, 18_168_000, "17m"),
        (21_000_000, 21_450_000, "15m"),
        (24_890_000, 24_990_000, "12m"),
        (28_000_000, 29_700_000, "10m"),
        (50_000_000, 54_000_000, "6m"),
        (144_000_000, 148_000_000, "2m"),
        (420_000_000, 450_000_000, "70cm"),
    ];
    for &(lo, hi, name) in BANDS {
        if hz >= lo && hz < hi {
            return name;
        }
    }
    "default"
}

/// Look up the TX attenuation for a given frequency using `levels`.
///
/// Returns the dB value for the matching band, or the `"default"` entry,
/// or `0.0` if neither is present.
pub fn attenuation_for_hz(levels: &HashMap<String, f32>, hz: u64) -> f32 {
    let band = freq_hz_to_band(hz);
    levels
        .get(band)
        .or_else(|| levels.get("default"))
        .copied()
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_bands_map_correctly() {
        assert_eq!(freq_hz_to_band(14_074_000), "20m");
        assert_eq!(freq_hz_to_band(7_074_000), "40m");
        assert_eq!(freq_hz_to_band(3_573_000), "80m");
        assert_eq!(freq_hz_to_band(10_136_000), "30m");
        assert_eq!(freq_hz_to_band(144_200_000), "2m");
    }

    #[test]
    fn unknown_frequency_returns_default() {
        assert_eq!(freq_hz_to_band(27_000_000), "default"); // CB band — not amateur
        assert_eq!(freq_hz_to_band(0), "default");
    }

    #[test]
    fn attenuation_lookup_uses_band_then_default() {
        let mut levels = HashMap::new();
        levels.insert("40m".into(), -6.0f32);
        levels.insert("default".into(), 0.0f32);

        assert!((attenuation_for_hz(&levels, 7_074_000) - (-6.0)).abs() < 1e-5);
        assert!((attenuation_for_hz(&levels, 14_074_000) - 0.0).abs() < 1e-5); // falls back to default
    }

    #[test]
    fn attenuation_returns_zero_when_no_entry() {
        let levels = HashMap::new();
        assert_eq!(attenuation_for_hz(&levels, 14_074_000), 0.0);
    }
}
