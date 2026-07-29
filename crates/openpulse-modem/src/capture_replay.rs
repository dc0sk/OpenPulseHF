//! Replay real recorded radio audio through the modem.
//!
//! **Why replay rather than model.** Every impairment the harness emulates — idle noise floor,
//! capture level, carrier offset, AGC, read cadence — is a *model* of a radio, and a model can be
//! wrong in exactly the way that hides a bug. A recorded capture cannot: it is the actual signal a
//! rig produced, with its real noise floor, real level, real spurs and real offset. The trade is
//! coverage for fidelity — a capture only covers the conditions that were recorded, and it goes
//! stale as the DSP changes — so replay complements the emulations rather than replacing them.
//!
//! The corpus lives in `crates/openpulse-modem/tests/captures/` with provenance recorded in the
//! README there. Captures are 8 kHz mono 16-bit (the modem's working rate) so they stay small
//! enough to live in the repository.
//!
//! A minimal RIFF/WAVE reader is implemented here on purpose: pulling in an audio-decoding
//! dependency to read the one format we record ourselves would be a poor trade.

use std::path::Path;

/// Real recorded audio, ready to hand to a receiver.
#[derive(Debug, Clone)]
pub struct Capture {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl Capture {
    /// Mean-square level — the quantity the engine's `EnergyGate` compares against, so this is the
    /// number that decides whether a capture is "hot" or "quiet" in the sense that matters.
    pub fn mean_sq(&self) -> f32 {
        if self.samples.is_empty() {
            return 0.0;
        }
        self.samples.iter().map(|s| s * s).sum::<f32>() / self.samples.len() as f32
    }

    /// Duration in seconds.
    pub fn duration_secs(&self) -> f32 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.samples.len() as f32 / self.sample_rate as f32
    }

    /// A slice of `n` samples starting at `from`, cycling if the capture is shorter than asked.
    ///
    /// Cycling matters: a few seconds of recorded idle can then pad an arbitrarily long synthetic
    /// capture, which is how a real noise floor gets used as the context around a frame.
    pub fn cycled(&self, from: usize, n: usize) -> Vec<f32> {
        if self.samples.is_empty() {
            return vec![0.0; n];
        }
        (0..n)
            .map(|i| self.samples[(from + i) % self.samples.len()])
            .collect()
    }
}

/// Read a 16-bit PCM RIFF/WAVE file. Multi-channel input is reduced to its first channel.
pub fn load_wav(path: impl AsRef<Path>) -> Result<Capture, String> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(format!("{}: not a RIFF/WAVE file", path.display()));
    }

    let u16at = |o: usize| u16::from_le_bytes([bytes[o], bytes[o + 1]]);
    let u32at = |o: usize| u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);

    let (mut channels, mut sample_rate, mut bits) = (0u16, 0u32, 0u16);
    let mut data: Option<(usize, usize)> = None;

    // Walk the chunk list rather than assuming a canonical 44-byte header: recorders interleave
    // LIST/fact chunks, and a fixed offset silently reads audio as metadata.
    let mut pos = 12usize;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32at(pos + 4) as usize;
        let body = pos + 8;
        if body + size > bytes.len() {
            break;
        }
        match id {
            b"fmt " if size >= 16 => {
                channels = u16at(body + 2);
                sample_rate = u32at(body + 4);
                bits = u16at(body + 14);
            }
            b"data" => data = Some((body, size)),
            _ => {}
        }
        pos = body + size + (size & 1); // chunks are word-aligned
    }

    let (off, size) = data.ok_or_else(|| format!("{}: no data chunk", path.display()))?;
    if bits != 16 {
        return Err(format!(
            "{}: expected 16-bit PCM, got {bits}-bit",
            path.display()
        ));
    }
    let channels = channels.max(1) as usize;

    let samples: Vec<f32> = bytes[off..off + size]
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
        .step_by(channels)
        .collect();

    Ok(Capture {
        samples,
        sample_rate,
    })
}

/// Load a capture from the repository corpus by file name.
pub fn load_corpus(name: &str) -> Result<Capture, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("captures")
        .join(name);
    load_wav(path)
}
