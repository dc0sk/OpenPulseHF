//! Automatic multi-notch filter — detects up to N narrowband CW interferers (QRM)
//! by spectral prominence and removes each with a second-order IIR notch biquad.
//!
//! A CW tone concentrates its energy in a handful of FFT bins, so it stands far
//! above its immediate neighbours; a modem signal spreads across many bins, so any
//! single bin is only marginally above the ones a few bins away. The detector keys
//! on that *local prominence*, which lets it null an interfering carrier without
//! notching a spread data signal of the same total power. The remaining failure
//! mode — a tone landing inside a narrowband signal's own main lobe — is physical,
//! not a detector bug: the notch then removes signal too.

use rustfft::{num_complex::Complex32, FftPlanner};

/// How notch centre frequencies are chosen each block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotchMode {
    /// Detect interferers automatically from each block's spectrum.
    Auto,
    /// Use the centre frequencies set via [`NotchBank::set_notch_freqs`] (oracle / manual).
    Fixed,
}

/// Tuning for [`NotchBank`].
#[derive(Debug, Clone)]
pub struct NotchParams {
    /// Audio sample rate (Hz).
    pub sample_rate: f32,
    /// Maximum simultaneous notches.
    pub max_notches: usize,
    /// Notch sharpness (BW ≈ f0 / q). Higher = narrower notch, less signal damage.
    pub q: f32,
    /// Detection FFT size (power of two recommended).
    pub fft_size: usize,
    /// A bin must exceed its local-floor (median of the surrounding window) by this many dB
    /// to count as a tone.
    pub prominence_db: f32,
    /// Half-width (Hz) of the window whose median sets each bin's local floor. A contiguous
    /// modem signal fills this window (high median → low prominence → not notched); an isolated
    /// CW tone sits over noise (low median → high prominence → notched). The median is also what
    /// makes the detector decline to notch a tone *inside* a signal's own band, where notching
    /// would remove signal too.
    pub floor_halfwidth_hz: f32,
    /// Merge / skip detected notches closer than this (Hz).
    pub min_spacing_hz: f32,
    /// Ignore detected peaks below this frequency (Hz).
    pub guard_lo_hz: f32,
    /// Ignore detected peaks above this frequency (Hz).
    pub guard_hi_hz: f32,
    /// Protected passband `[lo, hi]` (Hz): the receiver's own channel is never notched here,
    /// however prominent a peak looks. Set `lo >= hi` to disable. This is the receiver's
    /// legitimate self-knowledge — without it, blind detection notches the modem's own
    /// preamble / pulse spectral lines and destroys the signal.
    pub protect_lo_hz: f32,
    pub protect_hi_hz: f32,
}

impl Default for NotchParams {
    fn default() -> Self {
        Self {
            sample_rate: 8000.0,
            max_notches: 10,
            q: 25.0,
            fft_size: 4096,
            prominence_db: 14.0,
            floor_halfwidth_hz: 180.0,
            min_spacing_hz: 40.0,
            guard_lo_hz: 200.0,
            guard_hi_hz: 3600.0,
            protect_lo_hz: 0.0,
            protect_hi_hz: 0.0,
        }
    }
}

/// One second-order IIR notch (RBJ cookbook), normalised to unity passband gain.
#[derive(Debug, Clone, Copy)]
struct NotchBiquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
    f0: f32,
}

impl NotchBiquad {
    fn design(f0: f32, fs: f32, q: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * f0 / fs;
        let (sinw, cosw) = w0.sin_cos();
        let alpha = sinw / (2.0 * q.max(0.5));
        let a0 = 1.0 + alpha;
        Self {
            b0: 1.0 / a0,
            b1: -2.0 * cosw / a0,
            b2: 1.0 / a0,
            a1: -2.0 * cosw / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
            f0,
        }
    }

    /// Prime state to a constant so the cascade starts at steady state (no step transient):
    /// a notch passes DC unchanged, so output == input == `x` when primed this way.
    fn prime(&mut self, x: f32) {
        self.x1 = x;
        self.x2 = x;
        self.y1 = x;
        self.y2 = x;
    }

    #[inline]
    fn step(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// Quantization (Hz) for the persistence tracker's frequency keys.
const PERSIST_BIN_HZ: f32 = 10.0;

/// Tracks how persistently a tone appears across signal-absent (silence) blocks. A CW interferer
/// is present even when the receiver's own signal is not; the modem's own preamble/pulse spectral
/// lines are not. So a tone confirmed during silence is genuinely external — and can be notched
/// (out of band) or flagged for QSY (in band) without the per-block false-positive risk.
#[derive(Default)]
struct Persistence {
    /// Silence blocks a tone must appear in before it counts as confirmed. 0 = disabled.
    min_silence_hits: u32,
    /// freq key (Hz / PERSIST_BIN_HZ, rounded) → accumulated silence-hit score.
    tracks: std::collections::HashMap<i32, u32>,
}

/// Automatic multi-notch interference canceller.
pub struct NotchBank {
    params: NotchParams,
    mode: NotchMode,
    biquads: Vec<NotchBiquad>,
    planner: FftPlanner<f32>,
    window: Vec<f32>,
    persistence: Persistence,
}

impl NotchBank {
    /// Build a bank in [`NotchMode::Auto`].
    pub fn new(params: NotchParams) -> Self {
        let window = hann(params.fft_size);
        Self {
            params,
            mode: NotchMode::Auto,
            biquads: Vec::new(),
            planner: FftPlanner::new(),
            window,
            persistence: Persistence::default(),
        }
    }

    /// Switch detection mode.
    pub fn set_mode(&mut self, mode: NotchMode) {
        self.mode = mode;
    }

    /// Set fixed notch centre frequencies (oracle / manual placement).
    pub fn set_notch_freqs(&mut self, freqs_hz: &[f32]) {
        let fs = self.params.sample_rate;
        let q = self.params.q;
        self.biquads = freqs_hz
            .iter()
            .take(self.params.max_notches)
            .filter(|&&f| f > 0.0 && f < fs / 2.0)
            .map(|&f| NotchBiquad::design(f, fs, q))
            .collect();
    }

    /// Update the protected passband (Hz) the auto-detector must never notch — the receiver's
    /// own channel. Set `lo >= hi` to disable protection.
    pub fn set_protect_band(&mut self, lo_hz: f32, hi_hz: f32) {
        self.params.protect_lo_hz = lo_hz;
        self.params.protect_hi_hz = hi_hz;
    }

    /// Centre frequencies of the currently active notches (Hz).
    pub fn active_freqs(&self) -> Vec<f32> {
        self.biquads.iter().map(|b| b.f0).collect()
    }

    /// Enable persistence/silence tracking: a tone must appear in this many signal-absent
    /// (silence) blocks before it counts as a confirmed external interferer. 0 disables it
    /// (default). Confirmed externals are notched even if a single block's detection misses them,
    /// and in-band ones are surfaced via [`in_band_interferers`](Self::in_band_interferers).
    pub fn set_persistence(&mut self, min_silence_hits: u32) {
        self.persistence.min_silence_hits = min_silence_hits;
        if min_silence_hits == 0 {
            self.persistence.tracks.clear();
        }
    }

    /// Feed a captured block to the persistence tracker. The bank classifies the block itself:
    /// while the receiver's own (wideband) signal fills the protected band the block is skipped;
    /// otherwise the tones it contains are external (present independent of our transmission) and
    /// accrue toward confirmation. Requires a protected band to be set (via the engine per mode).
    pub fn observe(&mut self, block: &[f32]) {
        if self.persistence.min_silence_hits == 0 || block.is_empty() {
            return;
        }
        let mag_db = self.spectrum_db(block);
        if self.band_filled(&mag_db) {
            return; // our own signal is present — don't track its spectral lines as interferers
        }
        let peaks = self.peaks_from_spectrum(&mag_db, false);
        let cap = self.persistence.min_silence_hits * 2;
        // Decay every track by one, then credit the tones seen this silence block (+2 net).
        for v in self.persistence.tracks.values_mut() {
            *v = v.saturating_sub(1);
        }
        for f in peaks {
            let key = (f / PERSIST_BIN_HZ).round() as i32;
            let e = self.persistence.tracks.entry(key).or_insert(0);
            *e = (*e + 2).min(cap);
        }
        self.persistence.tracks.retain(|_, &mut v| v > 0);
    }

    /// Forget all accumulated persistence state (e.g. after a QSY to a new frequency, where the
    /// previously-confirmed interferers no longer apply).
    pub fn clear_persistence(&mut self) {
        self.persistence.tracks.clear();
    }

    fn confirmed_external(&self) -> Vec<f32> {
        if self.persistence.min_silence_hits == 0 {
            return Vec::new();
        }
        self.persistence
            .tracks
            .iter()
            .filter(|(_, &v)| v >= self.persistence.min_silence_hits)
            .map(|(&k, _)| k as f32 * PERSIST_BIN_HZ)
            .collect()
    }

    fn in_band(&self, f: f32) -> bool {
        self.params.protect_lo_hz < self.params.protect_hi_hz
            && f >= self.params.protect_lo_hz
            && f <= self.params.protect_hi_hz
    }

    /// Confirmed external interferers (Hz) — seen during silence, so independent of our TX.
    pub fn external_interferers(&self) -> Vec<f32> {
        self.confirmed_external()
    }

    /// Confirmed external interferers inside the protected band (Hz). A notch can't remove these
    /// without harming the signal, so they are QSY (move-frequency) candidates, not notch targets.
    pub fn in_band_interferers(&self) -> Vec<f32> {
        self.confirmed_external()
            .into_iter()
            .filter(|&f| self.in_band(f))
            .collect()
    }

    /// Process one block: in [`NotchMode::Auto`] re-detect interferers first, then apply the
    /// notch cascade. State is reset per block (each modem frame is an independent realisation),
    /// primed to the first sample so no startup step transient corrupts the preamble.
    pub fn process_block(&mut self, block: &[f32]) -> Vec<f32> {
        if block.is_empty() {
            return Vec::new();
        }
        if self.mode == NotchMode::Auto {
            let mut freqs = self.detect_freqs(block);
            // Add confirmed external interferers that sit out of band (persistence path): a notch
            // there is safe and these are robustly external even if this block's detection missed.
            for f in self.confirmed_external() {
                if !self.in_band(f)
                    && freqs
                        .iter()
                        .all(|&p| (p - f).abs() >= self.params.min_spacing_hz)
                {
                    freqs.push(f);
                }
            }
            freqs.truncate(self.params.max_notches);
            self.set_notch_freqs(&freqs);
        }
        if self.biquads.is_empty() {
            return block.to_vec();
        }
        let x0 = block[0];
        for bq in &mut self.biquads {
            bq.prime(x0);
        }
        let mut buf = block.to_vec();
        for bq in &mut self.biquads {
            for s in buf.iter_mut() {
                *s = bq.step(*s);
            }
        }
        buf
    }

    /// Detect up to `max_notches` narrowband interferers in a block by local spectral prominence,
    /// honouring the protected passband.
    pub fn detect_freqs(&mut self, block: &[f32]) -> Vec<f32> {
        self.detect_peaks(block, true)
    }

    /// Detect sharp spectral peaks. With `respect_protect`, peaks inside the protected passband are
    /// skipped; without it, every prominent tone (incl. in-band) is returned — used by the
    /// persistence tracker, which must see in-band interferers to flag them for QSY.
    fn detect_peaks(&mut self, block: &[f32], respect_protect: bool) -> Vec<f32> {
        if block.is_empty() {
            return Vec::new();
        }
        let mag_db = self.spectrum_db(block);
        self.peaks_from_spectrum(&mag_db, respect_protect)
    }

    /// Half-spectrum magnitude (dB) of a Hann-windowed, zero-padded block.
    fn spectrum_db(&mut self, block: &[f32]) -> Vec<f32> {
        let n = self.params.fft_size;
        let mut buf = vec![Complex32::new(0.0, 0.0); n];
        let take = block.len().min(n);
        for i in 0..take {
            buf[i] = Complex32::new(block[i] * self.window[i], 0.0);
        }
        self.planner.plan_fft_forward(n).process(&mut buf);
        (0..n / 2)
            .map(|k| 20.0 * (buf[k].norm() + 1e-9).log10())
            .collect()
    }

    /// True when the protected band is substantially *filled* — characteristic of the receiver's
    /// own wideband signal, as opposed to noise or a lone CW tone (which lights only a few bins,
    /// however loud). Used to gate persistence: tones are only tracked when our signal is absent.
    fn band_filled(&self, mag_db: &[f32]) -> bool {
        let (lo, hi) = (self.params.protect_lo_hz, self.params.protect_hi_hz);
        if lo >= hi {
            return false;
        }
        let bin_hz = self.params.sample_rate / self.params.fft_size as f32;
        let last = mag_db.len().saturating_sub(1);
        let glo = (self.params.guard_lo_hz / bin_hz).ceil() as usize;
        let ghi = ((self.params.guard_hi_hz / bin_hz).floor() as usize).min(last);
        if ghi <= glo {
            return false;
        }
        let mut floor: Vec<f32> = mag_db[glo..=ghi].to_vec();
        floor.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let noise = floor[floor.len() / 2];
        let plo = (lo / bin_hz).ceil() as usize;
        let phi = ((hi / bin_hz).floor() as usize).min(last);
        if phi <= plo {
            return false;
        }
        let active = (plo..=phi).filter(|&k| mag_db[k] > noise + 6.0).count();
        active as f32 / (phi - plo + 1) as f32 > 0.4
    }

    /// Pick interferer frequencies from a precomputed magnitude spectrum by local prominence.
    fn peaks_from_spectrum(&self, mag_db: &[f32], respect_protect: bool) -> Vec<f32> {
        let n = self.params.fft_size;
        let half = n / 2;
        let fs = self.params.sample_rate;
        let bin_hz = fs / n as f32;
        let lo_bin = (self.params.guard_lo_hz / bin_hz).ceil() as usize;
        let hi_bin = ((self.params.guard_hi_hz / bin_hz).floor() as usize).min(half - 1);
        // A tone's Hann main lobe is ~4 bins; require a local max over a little more than that.
        let inner = 6usize;
        // Window whose median is the local floor. Wide enough that a contiguous modem signal
        // fills it (high median) while an isolated tone does not (noise-floor median).
        let win = ((self.params.floor_halfwidth_hz / bin_hz).round() as usize).max(inner + 4);

        let mut cands: Vec<(f32, f32)> = Vec::new(); // (prominence_db, freq_hz)
        let mut ring: Vec<f32> = Vec::with_capacity(2 * win);
        for k in lo_bin..=hi_bin {
            if k < inner || k + inner >= half {
                continue;
            }
            // Never notch inside the receiver's own protected passband (unless detecting raw
            // peaks for the persistence tracker, which must see in-band tones to flag QSY).
            let f = k as f32 * bin_hz;
            if respect_protect && self.in_band(f) {
                continue;
            }
            // Local maximum within ±inner.
            if !(k - inner..=k + inner).all(|j| mag_db[k] >= mag_db[j]) {
                continue;
            }
            // Local floor = median of the surrounding window, excluding the peak's main lobe.
            ring.clear();
            let lo = k.saturating_sub(win);
            let hi = (k + win).min(half - 1);
            for (j, &m) in mag_db.iter().enumerate().take(hi + 1).skip(lo) {
                if j + inner < k || j > k + inner {
                    ring.push(m);
                }
            }
            if ring.is_empty() {
                continue;
            }
            ring.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let floor = ring[ring.len() / 2];
            let prom = mag_db[k] - floor;
            if prom >= self.params.prominence_db {
                cands.push((prom, f));
            }
        }
        // Strongest first, then greedily enforce min spacing.
        cands.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut picked: Vec<f32> = Vec::new();
        for (_, f) in cands {
            if picked.len() >= self.params.max_notches {
                break;
            }
            if picked
                .iter()
                .all(|&p| (p - f).abs() >= self.params.min_spacing_hz)
            {
                picked.push(f);
            }
        }
        picked
    }
}

fn hann(n: usize) -> Vec<f32> {
    if n <= 1 {
        return vec![1.0; n.max(1)];
    }
    (0..n)
        .map(|i| {
            let x = std::f32::consts::PI * i as f32 / (n - 1) as f32;
            x.sin().powi(2)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f32 = 8000.0;

    fn tone(freq: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / FS).sin())
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|&s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    #[test]
    fn detects_a_single_cw_tone() {
        let mut bank = NotchBank::new(NotchParams::default());
        // Wideband-ish noise + a strong tone at 1200 Hz.
        let mut sig = tone(1200.0, 1.0, 8192);
        for (i, s) in sig.iter_mut().enumerate() {
            *s += 0.1 * ((i as f32 * 0.7).sin() + (i as f32 * 3.1).cos());
        }
        let freqs = bank.detect_freqs(&sig);
        assert!(
            freqs.iter().any(|&f| (f - 1200.0).abs() < 30.0),
            "expected a notch near 1200 Hz, got {freqs:?}"
        );
    }

    #[test]
    fn fixed_notch_kills_its_tone() {
        let mut bank = NotchBank::new(NotchParams::default());
        bank.set_mode(NotchMode::Fixed);
        bank.set_notch_freqs(&[1500.0]);
        let sig = tone(1500.0, 1.0, 8192);
        let out = bank.process_block(&sig);
        // Ignore the first chunk (filter settling) when measuring suppression.
        let after = &out[1024..];
        let before = &sig[1024..];
        let supp_db = 20.0 * (rms(before) / rms(after).max(1e-9)).log10();
        assert!(
            supp_db > 25.0,
            "notch should suppress its tone by >25 dB, got {supp_db:.1} dB"
        );
    }

    #[test]
    fn passes_a_tone_far_from_the_notch() {
        let mut bank = NotchBank::new(NotchParams::default());
        bank.set_mode(NotchMode::Fixed);
        bank.set_notch_freqs(&[1500.0]);
        let sig = tone(800.0, 1.0, 8192);
        let out = bank.process_block(&sig);
        let after = &out[1024..];
        let before = &sig[1024..];
        let loss_db = 20.0 * (rms(before) / rms(after).max(1e-9)).log10();
        assert!(
            loss_db.abs() < 1.0,
            "a tone far from the notch should pass within 1 dB, lost {loss_db:.2} dB"
        );
    }

    #[test]
    fn protected_band_is_never_auto_notched() {
        // A strong tone inside the protected passband must be left alone (it is the receiver's
        // own channel); the same tone outside the band must be detected.
        let params = NotchParams {
            protect_lo_hz: 1300.0,
            protect_hi_hz: 1700.0,
            ..NotchParams::default()
        };
        let mut inside = NotchBank::new(params.clone());
        let sig_in = tone(1500.0, 1.0, 8192);
        assert!(
            !inside
                .detect_freqs(&sig_in)
                .iter()
                .any(|&f| (1300.0..=1700.0).contains(&f)),
            "a tone inside the protected band must not be notched"
        );

        let mut outside = NotchBank::new(params);
        let sig_out = tone(2400.0, 1.0, 8192);
        assert!(
            outside
                .detect_freqs(&sig_out)
                .iter()
                .any(|&f| (f - 2400.0).abs() < 30.0),
            "a tone outside the protected band must still be detected"
        );
    }

    /// A block that fills a band with many closely-spaced tones (stand-in for our wideband signal).
    fn wideband(lo: f32, hi: f32, n: usize) -> Vec<f32> {
        let mut v = vec![0.0f32; n];
        let mut f = lo;
        while f <= hi {
            for (i, s) in v.iter_mut().enumerate() {
                *s += 0.2 * (2.0 * std::f32::consts::PI * f * i as f32 / FS).sin();
            }
            f += 40.0;
        }
        v
    }

    #[test]
    fn persistence_confirms_an_external_tone_seen_during_silence() {
        let mut bank = NotchBank::new(NotchParams::default());
        bank.set_persistence(3);
        // A 1300 Hz tone over a quiet band (our signal absent) → confirmed external.
        let silence_tone = tone(1300.0, 0.5, 8192);
        for _ in 0..3 {
            bank.observe(&silence_tone);
        }
        assert!(
            bank.external_interferers()
                .iter()
                .any(|&f| (f - 1300.0).abs() < PERSIST_BIN_HZ),
            "tone seen across 3 silence blocks should be confirmed external, got {:?}",
            bank.external_interferers()
        );
    }

    #[test]
    fn persistence_flags_in_band_interferer_for_qsy() {
        let params = NotchParams {
            protect_lo_hz: 1200.0,
            protect_hi_hz: 1800.0,
            ..NotchParams::default()
        };
        let mut bank = NotchBank::new(params);
        bank.set_persistence(2);
        // A lone tone INSIDE the protected band over an otherwise-quiet band → QSY candidate.
        let in_band = tone(1500.0, 0.5, 8192);
        for _ in 0..2 {
            bank.observe(&in_band);
        }
        assert!(
            bank.in_band_interferers()
                .iter()
                .any(|&f| (f - 1500.0).abs() < PERSIST_BIN_HZ),
            "an in-band silence tone should be flagged for QSY, got {:?}",
            bank.in_band_interferers()
        );
    }

    #[test]
    fn persistence_ignores_blocks_while_our_signal_fills_the_band() {
        let params = NotchParams {
            protect_lo_hz: 1000.0,
            protect_hi_hz: 2000.0,
            ..NotchParams::default()
        };
        let mut bank = NotchBank::new(params);
        bank.set_persistence(2);
        // Our own signal fills 1000–2000 Hz → blocks are skipped, nothing is confirmed.
        let sig = wideband(1000.0, 2000.0, 8192);
        for _ in 0..5 {
            bank.observe(&sig);
        }
        assert!(bank.external_interferers().is_empty());
    }

    #[test]
    fn respects_max_notches() {
        let params = NotchParams {
            max_notches: 3,
            ..NotchParams::default()
        };
        let mut bank = NotchBank::new(params);
        bank.set_notch_freqs(&[600.0, 900.0, 1200.0, 1500.0, 1800.0]);
        assert_eq!(bank.active_freqs().len(), 3);
    }
}
