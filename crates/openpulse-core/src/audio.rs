use crate::error::AudioError;

// ── Device info ───────────────────────────────────────────────────────────────

/// Describes a physical or virtual audio device.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// System name of the device.
    pub name: String,
    /// `true` when the device can capture audio.
    pub is_input: bool,
    /// `true` when the device can play back audio.
    pub is_output: bool,
    /// `true` when this is the system default device for its direction.
    pub is_default: bool,
    /// Non-exhaustive list of sample rates the device accepts.
    pub supported_sample_rates: Vec<u32>,
}

// ── Hotplug-safe device resolution (REQ-DEV-01) ───────────────────────────────

/// Outcome of resolving a configured device selector against the live device list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceResolution {
    /// A unique device resolved; holds its current system name.
    Resolved(String),
    /// The selector matched more than one device — ambiguous, so we refuse to guess.
    Ambiguous(Vec<String>),
    /// No device matched the selector.
    NotFound,
}

/// Extract the stable ALSA `CARD=<token>` identifier from a device name, if present.
///
/// cpal/ALSA names embed `CARD=<name>` (e.g. `plughw:CARD=Device,DEV=0`); that token is independent of
/// the enumeration index, so it survives a device reorder that shifts `hw:N`.
fn alsa_card_token(name: &str) -> Option<&str> {
    let start = name.find("CARD=")? + "CARD=".len();
    let rest = &name[start..];
    let end = rest.find([',', ':']).unwrap_or(rest.len());
    let token = &rest[..end];
    (!token.is_empty()).then_some(token)
}

/// Resolve a configured device `selector` against the current device names, hotplug-safely.
///
/// Ladder (each tier only consulted if the previous found nothing), refusing to guess on ambiguity:
/// (1) **exact** system-name match (index/order-independent, so a pure reorder is handled here);
/// (2) **ALSA `CARD=` token** match — survives an index shift that renames `hw:0` → `hw:1`;
/// (3) **case-insensitive substring** either way — handles an OS rename like `USB Audio Device` gaining a
/// `(2)` suffix. An empty selector means "no device configured" → [`DeviceResolution::NotFound`] (the
/// caller uses the system default instead).
pub fn resolve_device(selector: &str, candidates: &[String]) -> DeviceResolution {
    if selector.is_empty() {
        return DeviceResolution::NotFound;
    }
    // Tier 1: exact.
    if candidates.iter().any(|c| c == selector) {
        return DeviceResolution::Resolved(selector.to_string());
    }
    // Tier 2: ALSA CARD= token.
    if let Some(sel_token) = alsa_card_token(selector) {
        let hits: Vec<String> = candidates
            .iter()
            .filter(|c| alsa_card_token(c) == Some(sel_token))
            .cloned()
            .collect();
        match hits.len() {
            1 => return DeviceResolution::Resolved(hits.into_iter().next().unwrap_or_default()),
            n if n > 1 => return DeviceResolution::Ambiguous(hits),
            _ => {}
        }
    }
    // Tier 3: case-insensitive substring, either direction.
    let sel_lower = selector.to_lowercase();
    let hits: Vec<String> = candidates
        .iter()
        .filter(|c| {
            let c_lower = c.to_lowercase();
            c_lower.contains(&sel_lower) || sel_lower.contains(&c_lower)
        })
        .cloned()
        .collect();
    match hits.len() {
        1 => DeviceResolution::Resolved(hits.into_iter().next().unwrap_or_default()),
        n if n > 1 => DeviceResolution::Ambiguous(hits),
        _ => DeviceResolution::NotFound,
    }
}

// ── Stream configuration ──────────────────────────────────────────────────────

/// Parameters used when opening an audio stream.
#[derive(Debug, Clone)]
pub struct AudioConfig {
    /// Desired sample rate in Hz.
    pub sample_rate: u32,
    /// Number of channels (1 = mono is sufficient for radio work).
    pub channels: u16,
    /// Optional driver buffer size hint in frames.
    pub buffer_size: Option<u32>,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: 8000,
            channels: 1,
            buffer_size: None,
        }
    }
}

// ── Stream traits ─────────────────────────────────────────────────────────────

/// An open audio capture stream.
pub trait AudioInputStream {
    /// Block until at least one sample is available, then return all buffered
    /// samples normalised to `−1.0 … +1.0`.
    fn read(&mut self) -> Result<Vec<f32>, AudioError>;

    /// Release underlying resources.
    fn close(self: Box<Self>);
}

/// An open audio playback stream.
pub trait AudioOutputStream {
    /// Write `samples` (normalised `−1.0 … +1.0`) to the device.
    fn write(&mut self, samples: &[f32]) -> Result<(), AudioError>;

    /// Ensure all buffered samples have been submitted to the driver.
    fn flush(&mut self) -> Result<(), AudioError>;

    /// Release underlying resources.
    fn close(self: Box<Self>);
}

// ── Backend trait ─────────────────────────────────────────────────────────────

/// An audio subsystem backend (ALSA, PipeWire, CoreAudio, WASAPI, Loopback …).
pub trait AudioBackend: Send + Sync {
    /// Human-readable backend name.
    fn name(&self) -> &str;

    /// Enumerate all available devices.
    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AudioError>;

    /// Open a capture stream.  Pass `None` for `device` to use the default.
    fn open_input(
        &self,
        device: Option<&str>,
        config: &AudioConfig,
    ) -> Result<Box<dyn AudioInputStream>, AudioError>;

    /// Open a playback stream.  Pass `None` for `device` to use the default.
    fn open_output(
        &self,
        device: Option<&str>,
        config: &AudioConfig,
    ) -> Result<Box<dyn AudioOutputStream>, AudioError>;

    /// Open a stereo I/Q output stream (left = I, right = Q).
    ///
    /// Returns `None` when the backend does not support I/Q output; the caller
    /// should fall back to [`open_output`](Self::open_output) in that case.
    fn open_iq_output(
        &self,
        _device: Option<&str>,
        _config: &AudioConfig,
    ) -> Option<Result<Box<dyn AudioIqOutputStream>, AudioError>> {
        None
    }
}

// ── I/Q stream trait ──────────────────────────────────────────────────────────

/// An open stereo I/Q playback stream (left = I, right = Q).
///
/// Used when the audio backend supports stereo output suitable for direct
/// SDR upconversion.  Both sample slices passed to `write_iq` must have the
/// same length.
pub trait AudioIqOutputStream {
    /// Write baseband I and Q samples.  Both slices must have the same length.
    fn write_iq(&mut self, i: &[f32], q: &[f32]) -> Result<(), AudioError>;

    /// Flush all buffered samples to the device.
    fn flush(&mut self) -> Result<(), AudioError>;

    /// Release underlying resources.
    fn close(self: Box<Self>);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn empty_selector_is_not_found() {
        assert_eq!(
            resolve_device("", &names(&["USB Audio Device"])),
            DeviceResolution::NotFound
        );
    }

    #[test]
    fn exact_match_wins_and_survives_reorder() {
        // Acceptance (REQ-DEV-01): a configured device resolves after a simulated reorder — exact match
        // is index-independent, so the same name resolves regardless of its position in the list.
        let ordered = names(&["Built-in", "USB Audio Device", "HDMI"]);
        let reordered = names(&["HDMI", "USB Audio Device", "Built-in"]);
        assert_eq!(
            resolve_device("USB Audio Device", &ordered),
            DeviceResolution::Resolved("USB Audio Device".into())
        );
        assert_eq!(
            resolve_device("USB Audio Device", &reordered),
            DeviceResolution::Resolved("USB Audio Device".into())
        );
    }

    #[test]
    fn alsa_card_token_survives_an_index_rename() {
        // hw:0 → hw:1 after a reorder, but CARD= is stable.
        let before = names(&["plughw:CARD=Device,DEV=0", "plughw:CARD=PCH,DEV=0"]);
        let after = names(&["plughw:CARD=PCH,DEV=0", "plughw:CARD=Device,DEV=0"]);
        // Selector was captured as the old exact name; only the order changed → exact still matches.
        assert_eq!(
            resolve_device("plughw:CARD=Device,DEV=0", &after),
            DeviceResolution::Resolved("plughw:CARD=Device,DEV=0".into())
        );
        // If the non-CARD part changed (e.g. DEV index), the CARD token still resolves it.
        let renamed = names(&["plughw:CARD=Device,DEV=1", "plughw:CARD=PCH,DEV=0"]);
        assert_eq!(
            resolve_device("plughw:CARD=Device,DEV=0", &renamed),
            DeviceResolution::Resolved("plughw:CARD=Device,DEV=1".into())
        );
        let _ = before;
    }

    #[test]
    fn substring_handles_an_os_rename() {
        // The classic "(2)" suffix a hotplug adds.
        assert_eq!(
            resolve_device("USB Audio Device", &names(&["USB Audio Device (2)"])),
            DeviceResolution::Resolved("USB Audio Device (2)".into())
        );
        // And the reverse: the config has the suffixed name, the device came back plain.
        assert_eq!(
            resolve_device("USB Audio Device (2)", &names(&["USB Audio Device"])),
            DeviceResolution::Resolved("USB Audio Device".into())
        );
    }

    #[test]
    fn ambiguous_match_refuses_to_guess() {
        let cands = names(&["USB Audio Device (1)", "USB Audio Device (2)"]);
        match resolve_device("USB Audio Device", &cands) {
            DeviceResolution::Ambiguous(mut hits) => {
                hits.sort();
                assert_eq!(hits, cands);
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn unrelated_selector_is_not_found() {
        assert_eq!(
            resolve_device("Nonexistent Rig", &names(&["Built-in", "HDMI"])),
            DeviceResolution::NotFound
        );
    }
}
