//! Morse-code CW station identification audio.
//!
//! Generates keyed-sine audio for a callsign so a station can append a CW ID to its
//! digital identification (the ARDOP `CWID` option). Pure generation over an explicit
//! sample rate — no audio I/O — so it is deterministic and unit-testable.

/// Morse element sequence (`.`/`-`) for one character, or `""` if unsupported.
pub fn morse(c: char) -> &'static str {
    match c.to_ascii_uppercase() {
        'A' => ".-",
        'B' => "-...",
        'C' => "-.-.",
        'D' => "-..",
        'E' => ".",
        'F' => "..-.",
        'G' => "--.",
        'H' => "....",
        'I' => "..",
        'J' => ".---",
        'K' => "-.-",
        'L' => ".-..",
        'M' => "--",
        'N' => "-.",
        'O' => "---",
        'P' => ".--.",
        'Q' => "--.-",
        'R' => ".-.",
        'S' => "...",
        'T' => "-",
        'U' => "..-",
        'V' => "...-",
        'W' => ".--",
        'X' => "-..-",
        'Y' => "-.--",
        'Z' => "--..",
        '0' => "-----",
        '1' => ".----",
        '2' => "..---",
        '3' => "...--",
        '4' => "....-",
        '5' => ".....",
        '6' => "-....",
        '7' => "--...",
        '8' => "---..",
        '9' => "----.",
        '/' => "-..-.",
        '?' => "..--..",
        '.' => ".-.-.-",
        ',' => "--..--",
        '-' => "-....-",
        _ => "",
    }
}

/// Space-separated Morse for a whole string (`/` marks a word gap). For inspection/tests.
pub fn morse_code(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for ch in text.chars() {
        if ch == ' ' {
            out.push("/".to_string());
        } else {
            let m = morse(ch);
            if !m.is_empty() {
                out.push(m.to_string());
            }
        }
    }
    out.join(" ")
}

/// CW identification tone generator.
#[derive(Debug, Clone, Copy)]
pub struct CwId {
    /// Speed in words per minute (PARIS standard: dot = 1.2 / wpm seconds).
    pub wpm: u32,
    /// Sidetone frequency in Hz.
    pub tone_hz: f32,
    /// Peak amplitude (0..=1).
    pub amplitude: f32,
}

impl Default for CwId {
    fn default() -> Self {
        Self {
            wpm: 18,
            tone_hz: 700.0,
            amplitude: 0.6,
        }
    }
}

impl CwId {
    /// Render `text` to keyed-sine audio at `sample_rate` Hz. Standard Morse spacing:
    /// dash = 3 units, intra-character gap = 1 unit, inter-character = 3 units, word = 7
    /// units (a space in `text`). Each keyed element has a short raised-cosine ramp to
    /// avoid key clicks; ramps live *inside* the element so element/gap timing stays exact.
    pub fn samples(&self, text: &str, sample_rate: u32) -> Vec<f32> {
        let unit = (1.2 / self.wpm.max(1) as f32 * sample_rate as f32).round() as usize;
        if unit == 0 {
            return Vec::new();
        }
        let ramp = (unit / 10).max(1);
        let mut out: Vec<f32> = Vec::new();

        let push_silence =
            |out: &mut Vec<f32>, units: usize| out.extend(std::iter::repeat_n(0.0, units * unit));
        let two_pi = std::f32::consts::TAU;
        let push_tone = |out: &mut Vec<f32>, len: usize| {
            for k in 0..len {
                // Raised-cosine attack/decay within the keyed duration.
                let env = if k < ramp {
                    0.5 - 0.5 * (std::f32::consts::PI * k as f32 / ramp as f32).cos()
                } else if k >= len.saturating_sub(ramp) {
                    let d = len - k;
                    0.5 - 0.5 * (std::f32::consts::PI * d as f32 / ramp as f32).cos()
                } else {
                    1.0
                };
                let s = self.amplitude
                    * env
                    * (two_pi * self.tone_hz * k as f32 / sample_rate as f32).sin();
                out.push(s);
            }
        };

        let mut first_char = true;
        for ch in text.chars() {
            if ch == ' ' {
                push_silence(&mut out, 7);
                first_char = true;
                continue;
            }
            let code = morse(ch);
            if code.is_empty() {
                continue;
            }
            if !first_char {
                push_silence(&mut out, 3);
            }
            first_char = false;
            for (i, e) in code.chars().enumerate() {
                if i > 0 {
                    push_silence(&mut out, 1);
                }
                push_tone(&mut out, if e == '-' { 3 * unit } else { unit });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn morse_table_maps_known_characters() {
        assert_eq!(morse('A'), ".-");
        assert_eq!(morse('S'), "...");
        assert_eq!(morse('O'), "---");
        assert_eq!(morse('5'), ".....");
        assert_eq!(morse('0'), "-----");
        assert_eq!(morse('/'), "-..-.");
        assert_eq!(morse('a'), ".-", "case-insensitive");
        assert_eq!(morse('#'), "", "unsupported → empty");
    }

    #[test]
    fn morse_code_joins_and_marks_word_gaps() {
        assert_eq!(morse_code("SOS"), "... --- ...");
        assert_eq!(morse_code("A B"), ".- / -...");
        assert_eq!(
            morse_code("K7#Q"),
            "-.- --... --.-",
            "unsupported chars dropped"
        );
    }

    #[test]
    fn single_dot_is_exactly_one_unit() {
        let fs = 8000;
        let cw = CwId::default(); // 18 wpm
        let unit = (1.2 / 18.0 * fs as f32).round() as usize;
        assert_eq!(cw.samples("E", fs).len(), unit, "E is one dot = one unit");
    }

    #[test]
    fn inter_character_gap_is_three_units() {
        let fs = 8000;
        let cw = CwId::default();
        let unit = (1.2 / 18.0 * fs as f32).round() as usize;
        // "EE" = dot + 3-unit inter-char gap + dot = 5 units.
        assert_eq!(cw.samples("EE", fs).len(), 5 * unit);
    }

    #[test]
    fn word_gap_is_seven_units() {
        let fs = 8000;
        let cw = CwId::default();
        let unit = (1.2 / 18.0 * fs as f32).round() as usize;
        // "E E" = dot + 7-unit word gap + dot = 9 units (the space replaces the inter-char gap).
        assert_eq!(cw.samples("E E", fs).len(), 9 * unit);
    }

    #[test]
    fn dash_is_three_units() {
        let fs = 8000;
        let cw = CwId::default();
        let unit = (1.2 / 18.0 * fs as f32).round() as usize;
        assert_eq!(
            cw.samples("T", fs).len(),
            3 * unit,
            "T is one dash = three units"
        );
    }

    #[test]
    fn samples_respect_amplitude_bounds_and_are_nonempty() {
        let cw = CwId::default();
        let s = cw.samples("W1AW", 8000);
        assert!(!s.is_empty());
        assert!(
            s.iter().all(|v| v.abs() <= cw.amplitude + 1e-6),
            "within peak amplitude"
        );
        assert!(s.iter().any(|v| v.abs() > 0.1), "has audible tone energy");
    }

    #[test]
    fn empty_or_unsupported_text_yields_no_samples() {
        let cw = CwId::default();
        assert!(cw.samples("", 8000).is_empty());
        assert!(cw.samples("###", 8000).is_empty());
    }
}
