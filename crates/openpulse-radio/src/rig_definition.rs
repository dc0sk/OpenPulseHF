//! TOML rig-definition structs for `GenericSerialCat`.
//!
//! A rig definition describes serial framing and named commands.  Commands are
//! space-separated hex byte strings; tokens in `{braces}` are substituted at
//! call time.
//!
//! # Supported substitution tokens
//!
//! | Token | Description |
//! |---|---|
//! | `{freq_bcd_le5}` | 5-byte BCD little-endian frequency in Hz (Icom CI-V) |
//! | `{freq_bcd4_be}` | 4-byte BCD big-endian frequency in Hz (Yaesu) |
//! | `{<param>}` | Single hex byte looked up from the `[params]` table |

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Top-level rig definition loaded from a `.toml` file.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RigDefinition {
    pub rig: RigMeta,
    #[serde(default)]
    pub params: HashMap<String, String>,
    #[serde(default)]
    pub commands: HashMap<String, CommandDef>,
}

/// Serial framing metadata.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RigMeta {
    pub model: String,
    #[serde(default)]
    pub description: String,
    pub baud: u32,
    #[serde(default = "default_data_bits")]
    pub data_bits: u8,
    #[serde(default = "default_stop_bits")]
    pub stop_bits: u8,
    #[serde(default = "default_parity")]
    pub parity: String,
}

fn default_data_bits() -> u8 {
    8
}
fn default_stop_bits() -> u8 {
    1
}
fn default_parity() -> String {
    "none".into()
}

/// Definition of one named command (e.g. `ptt_on`, `set_frequency`).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CommandDef {
    /// Space-separated hex byte string, optionally containing `{token}` slots.
    pub send: String,
    /// Number of response bytes to read after sending; 0 = no readback.
    #[serde(default)]
    pub response_bytes: usize,
    /// How to extract a scalar value from the response (for read commands).
    pub response_extract: Option<ResponseExtract>,
}

/// Describes how to parse a numeric value out of a command response.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResponseExtract {
    /// Byte offset in the response to start extracting from.
    pub offset: usize,
    /// Number of bytes to extract.
    pub length: usize,
    /// Encoding of the extracted bytes: `"bcd_le"`, `"bcd_be"`, `"u32_le"`.
    pub encoding: String,
}

impl RigDefinition {
    /// Parse a TOML string into a `RigDefinition`.
    pub fn from_toml(src: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(src)
    }
}

// ── BCD helpers ───────────────────────────────────────────────────────────────

/// Encode `hz` as 5-byte BCD little-endian (Icom CI-V style).
///
/// Each nibble represents one decimal digit.  The least-significant digit
/// occupies the low nibble of byte 0.
///
/// Example: 14_074_000 Hz → `[0x00, 0x40, 0x07, 0x14, 0x00]`
pub fn encode_freq_bcd_le5(hz: u64) -> [u8; 5] {
    let mut out = [0u8; 5];
    let mut v = hz;
    for byte in &mut out {
        let lo = (v % 10) as u8;
        v /= 10;
        let hi = (v % 10) as u8;
        v /= 10;
        *byte = (hi << 4) | lo;
    }
    out
}

/// Decode 5-byte BCD little-endian into Hz.
pub fn decode_freq_bcd_le5(bytes: &[u8; 5]) -> u64 {
    let mut hz = 0u64;
    let mut mul = 1u64;
    for &byte in bytes {
        hz += (byte & 0x0F) as u64 * mul;
        mul *= 10;
        hz += ((byte >> 4) & 0x0F) as u64 * mul;
        mul *= 10;
    }
    hz
}

/// Encode `hz` as 4-byte BCD big-endian (Yaesu FT-817 style).
///
/// Each byte stores two decimal digits (high nibble = more significant).
/// Bytes are ordered most-significant first.
///
/// Example: 14_074_000 Hz → `[0x14, 0x07, 0x40, 0x00]`
pub fn encode_freq_bcd4_be(hz: u64) -> [u8; 4] {
    let mut out = [0u8; 4];
    let mut v = hz;
    for byte in out.iter_mut().rev() {
        let lo = (v % 10) as u8;
        v /= 10;
        let hi = (v % 10) as u8;
        v /= 10;
        *byte = (hi << 4) | lo;
    }
    out
}

/// Expand a command `send` template into a concrete byte sequence.
///
/// Substitutes `{token}` slots using `params` and built-in frequency tokens.
/// `freq_hz` is `None` for commands that don't take a frequency argument.
pub fn expand_command(
    template: &str,
    params: &HashMap<String, String>,
    freq_hz: Option<u64>,
) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for token in template.split_whitespace() {
        if let Some(inner) = token.strip_prefix('{').and_then(|t| t.strip_suffix('}')) {
            match inner {
                "freq_bcd_le5" => {
                    let hz = freq_hz
                        .ok_or_else(|| "freq_bcd_le5 requires a frequency argument".to_string())?;
                    out.extend_from_slice(&encode_freq_bcd_le5(hz));
                }
                "freq_bcd4_be" => {
                    let hz = freq_hz
                        .ok_or_else(|| "freq_bcd4_be requires a frequency argument".to_string())?;
                    out.extend_from_slice(&encode_freq_bcd4_be(hz));
                }
                name => {
                    let hex = params
                        .get(name)
                        .ok_or_else(|| format!("unknown param '{name}'"))?;
                    let byte = u8::from_str_radix(hex.trim_start_matches("0x"), 16)
                        .map_err(|e| format!("invalid param '{name}': {e}"))?;
                    out.push(byte);
                }
            }
        } else {
            let byte = u8::from_str_radix(token, 16)
                .map_err(|e| format!("invalid hex byte '{token}': {e}"))?;
            out.push(byte);
        }
    }
    Ok(out)
}

/// Decode a scalar value from `bytes` using the given `encoding`.
///
/// Supported encodings: `"bcd_le"` (BCD little-endian), `"bcd_be"` (BCD big-endian),
/// `"u32_le"` (4-byte unsigned little-endian).
pub fn decode_response_value(bytes: &[u8], encoding: &str) -> Result<u64, String> {
    match encoding {
        "bcd_le" => {
            let mut hz = 0u64;
            let mut mul = 1u64;
            for &b in bytes {
                hz += (b & 0x0F) as u64 * mul;
                mul *= 10;
                hz += ((b >> 4) & 0x0F) as u64 * mul;
                mul *= 10;
            }
            Ok(hz)
        }
        "bcd_be" => {
            let mut hz = 0u64;
            for &b in bytes {
                hz = hz * 100 + ((b >> 4) & 0x0F) as u64 * 10 + (b & 0x0F) as u64;
            }
            Ok(hz)
        }
        "u32_le" if bytes.len() >= 4 => {
            Ok(u32::from_le_bytes(bytes[..4].try_into().unwrap()) as u64)
        }
        other => Err(format!("unknown response encoding '{other}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bcd_le5_round_trip_14mhz() {
        let hz = 14_074_000u64;
        let enc = encode_freq_bcd_le5(hz);
        assert_eq!(decode_freq_bcd_le5(&enc), hz);
    }

    #[test]
    fn bcd_le5_encoding_bytes() {
        // 14 074 000 Hz: digits (from LSB) 0,0,0,4,7,0,4,1,0,0
        // Each byte: hi nibble = more-significant of pair, lo = less-significant
        // → bytes [0x00, 0x40, 0x07, 0x14, 0x00]
        let enc = encode_freq_bcd_le5(14_074_000);
        assert_eq!(enc, [0x00, 0x40, 0x07, 0x14, 0x00]);
    }

    #[test]
    fn bcd4_be_encoding_14mhz() {
        // 14_074_000 Hz → [0x14, 0x07, 0x40, 0x00]
        let enc = encode_freq_bcd4_be(14_074_000);
        assert_eq!(enc, [0x14, 0x07, 0x40, 0x00]);
    }

    #[test]
    fn expand_ptt_on_icom() {
        let mut params = HashMap::new();
        params.insert("addr".into(), "94".into());
        params.insert("ctrl".into(), "E0".into());
        let bytes = expand_command("FE FE {addr} {ctrl} 1C 00 01 FD", &params, None).unwrap();
        assert_eq!(bytes, vec![0xFE, 0xFE, 0x94, 0xE0, 0x1C, 0x00, 0x01, 0xFD]);
    }

    #[test]
    fn expand_set_frequency_icom() {
        let mut params = HashMap::new();
        params.insert("addr".into(), "94".into());
        params.insert("ctrl".into(), "E0".into());
        let bytes = expand_command(
            "FE FE {addr} {ctrl} 00 {freq_bcd_le5} FD",
            &params,
            Some(14_074_000),
        )
        .unwrap();
        assert_eq!(
            bytes,
            vec![0xFE, 0xFE, 0x94, 0xE0, 0x00, 0x00, 0x40, 0x07, 0x14, 0x00, 0xFD]
        );
    }

    #[test]
    fn expand_set_frequency_yaesu() {
        let bytes = expand_command("{freq_bcd4_be} 01", &HashMap::new(), Some(14_074_000)).unwrap();
        assert_eq!(bytes, vec![0x14, 0x07, 0x40, 0x00, 0x01]);
    }
}
