//! Shared fixture for the OFDM preamble-detect gates (`docs/dev/design/ofdm-preamble-detect.md`):
//! an engine holding a `fast` session at one rung, fed through `accumulate_capture` in daemon-sized
//! reads, with every flushed burst recorded and decoded on the OTA arm.

#![allow(dead_code)] // each gate binary uses part of this

use openpulse_audio::LoopbackBackend;
use openpulse_core::error::ModemError;
use openpulse_core::plugin::{
    CarrierDetector, FrameGeometry, ModulationConfig, ModulationPlugin, PluginInfo,
};
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::capture_replay::load_corpus;
use openpulse_modem::ModemEngine;

/// Session id for the OTA decodes.
pub const SESSION: &str = "PDETECT";

/// The modem rate.
pub const RATE: usize = 8000;

/// OFDM52's symbol period, by reference to the plugin's geometry.
pub fn ofdm_symbol() -> usize {
    ofdm_plugin::OfdmPlugin::new()
        .frame_geometry(&ModulationConfig {
            mode: "OFDM52".into(),
            ..ModulationConfig::default()
        })
        .expect("OFDM52 geometry")
        .symbol_period_samples
}

/// The OFDM plugin with its carrier detector withheld: P off, everything else identical.
pub struct OfdmWithoutDetector(pub ofdm_plugin::OfdmPlugin);

impl ModulationPlugin for OfdmWithoutDetector {
    fn info(&self) -> &PluginInfo {
        self.0.info()
    }
    fn modulate(&self, d: &[u8], c: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
        self.0.modulate(d, c)
    }
    fn demodulate(&self, s: &[f32], c: &ModulationConfig) -> Result<Vec<u8>, ModemError> {
        self.0.demodulate(s, c)
    }
    fn demodulate_soft(&self, s: &[f32], c: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
        self.0.demodulate_soft(s, c)
    }
    fn frame_geometry(&self, c: &ModulationConfig) -> Option<FrameGeometry> {
        self.0.frame_geometry(c)
    }
    fn supports_soft_demod(&self, mode: &str) -> bool {
        self.0.supports_soft_demod(mode)
    }
    fn estimate_snr_db(&self, s: &[f32], c: &ModulationConfig) -> Option<f32> {
        self.0.estimate_snr_db(s, c)
    }
    fn occupied_bandwidth_hz(&self, mode: &str) -> Option<f32> {
        self.0.occupied_bandwidth_hz(mode)
    }
    fn modulate_iq(
        &self,
        d: &[u8],
        c: &ModulationConfig,
    ) -> Result<(Vec<f32>, Vec<f32>), ModemError> {
        self.0.modulate_iq(d, c)
    }
    fn carrier_detector(&self, _c: &ModulationConfig) -> Option<Box<dyn CarrierDetector>> {
        None
    }
}

/// An engine with the `hpx_hf` plugins, a `fast` session locked at `level`, and P on or off.
pub fn engine(level: SpeedLevel, p_on: bool) -> ModemEngine {
    let lb = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(lb.clone_shared()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .expect("bpsk");
    e.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
        .expect("qpsk");
    e.register_plugin(Box::new(mfsk16_plugin::Mfsk16Plugin::new()))
        .expect("mfsk16");
    e.register_plugin(Box::new(fsk4_plugin::Fsk4Plugin::new()))
        .expect("fsk4");
    if p_on {
        e.register_plugin(Box::new(ofdm_plugin::OfdmPlugin::new()))
            .expect("ofdm");
    } else {
        e.register_plugin(Box::new(
            OfdmWithoutDetector(ofdm_plugin::OfdmPlugin::new()),
        ))
        .expect("ofdm without detector");
    }
    e.start_ota_session(SessionProfile::fast());
    e.ota_lock_level(level);
    e
}

/// One flushed burst, on the feed's timeline.
#[derive(Clone, PartialEq)]
pub struct Flush {
    /// Timeline index of the burst's first sample.
    pub start: usize,
    pub len: usize,
    pub lead: usize,
    /// Bit pattern of the samples, so two feeds' flushes compare exactly.
    pub digest: u64,
    /// `(mode, payload)` the OTA arm decoded, when `decode` was asked.
    pub decoded: Option<(String, Vec<u8>)>,
    /// The burst itself.
    pub samples: Vec<f32>,
}

impl std::fmt::Debug for Flush {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Flush {{ start: {}, len: {}, lead: {}, decoded: {:?} }}",
            self.start,
            self.len,
            self.lead,
            self.decoded.as_ref().map(|(m, p)| (m, p.len()))
        )
    }
}

/// A stretch of contiguous audio at `at` on the feed's timeline. A segment that does not follow the
/// previous one directly is a stream break: the feed calls `abandon_burst` before it, as the daemon
/// does when it drops its capture stream around its own transmission.
pub struct Segment<'a> {
    pub at: usize,
    pub samples: &'a [f32],
}

fn digest(x: &[f32]) -> u64 {
    x.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, v| {
        (h ^ u64::from(v.to_bits())).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Feed `segments` in `read`-sample reads through `accumulate_capture`; return every flush.
pub fn feed(
    e: &mut ModemEngine,
    segments: &[Segment<'_>],
    read: usize,
    decode: bool,
) -> Vec<Flush> {
    let mut out = Vec::new();
    let mut end: Option<usize> = None;
    for seg in segments {
        if end.is_some_and(|x| x != seg.at) {
            e.abandon_burst();
        }
        let mut pos = seg.at;
        for chunk in seg.samples.chunks(read) {
            pos += chunk.len();
            let Ok(Some(b)) = e.accumulate_capture(Some("OFDM52"), chunk.to_vec()) else {
                continue;
            };
            // The flushing read is not in the burst: it ends where that read starts.
            let start = pos - chunk.len() - b.samples.len();
            let lead = e.last_flush_lead();
            let decoded = if decode {
                e.ota_decode_burst(&b, SESSION, None)
                    .ok()
                    .and_then(|r| r.payload.zip(r.mode))
                    .map(|(p, m)| (m, p))
            } else {
                None
            };
            out.push(Flush {
                start,
                len: b.samples.len(),
                lead,
                digest: digest(&b.samples),
                decoded,
                samples: b.samples,
            });
        }
        end = Some(pos);
    }
    out
}

/// A committed capture's samples.
pub fn capture(name: &str) -> Vec<f32> {
    load_corpus(name)
        .unwrap_or_else(|e| panic!("corpus file {name} must load: {e}"))
        .samples
}

/// Seconds to samples.
pub fn s(t: f64) -> usize {
    (t * RATE as f64).round() as usize
}

/// One committed f2g episode: A (IC-9700, AGC FAST) hearing two of B's SL7 frames.
pub struct Episode {
    pub file: &'static str,
    /// Preamble starts in the excerpt, seconds (measured: the design note's list minus the cut).
    pub onsets: [f64; 2],
    /// A's own keyed intervals in the excerpt, seconds: the daemon drops its stream there.
    pub own_tx: [(f64, f64); 2],
}

/// The three f2g episodes (`tests/captures/README.md`).
pub const EPISODES: [Episode; 3] = [
    Episode {
        file: "ic9700-ofdm52-f2g-agc-1.wav",
        onsets: [9.200, 21.007],
        own_tx: [(0.527, 1.527), (7.427, 8.427)],
    },
    Episode {
        file: "ic9700-ofdm52-f2g-agc-2.wav",
        onsets: [9.200, 20.967],
        own_tx: [(0.514, 1.614), (7.564, 7.864)],
    },
    Episode {
        file: "ic9700-ofdm52-f2g-agc-3.wav",
        onsets: [9.200, 21.011],
        own_tx: [(0.321, 1.371), (7.121, 8.171)],
    },
];

/// SL7's on-air frame length (≤ 200 B, soft-concatenated): 12 096 samples, from the design note.
pub const SL7_FRAME: usize = 12_096;

/// Where the excerpt sits on the feed's timeline: after the warm-up, with room.
pub const EXCERPT_AT: usize = 1_000_000;

impl Episode {
    /// The gap hiss between the two frames: 0.3 s clear of each.
    pub fn gap<'a>(&self, x: &'a [f32]) -> &'a [f32] {
        &x[s(self.onsets[0]) + SL7_FRAME + s(0.3)..s(self.onsets[1]) - s(0.3)]
    }

    /// The feed: the gap hiss to warm the floor (≥ 9 s), the excerpt with A's own transmissions
    /// cut out, then the gap hiss again so the last frame's burst can flush at any read size.
    pub fn segments<'a>(&self, x: &'a [f32], cold: bool) -> Vec<Segment<'a>> {
        let mut v = Vec::new();
        if !cold {
            v.push(Segment {
                at: 0,
                samples: self.gap(x),
            });
        }
        let mut from = 0usize;
        for &(a, b) in &self.own_tx {
            v.push(Segment {
                at: EXCERPT_AT + from,
                samples: &x[from..s(a)],
            });
            from = s(b);
        }
        v.push(Segment {
            at: EXCERPT_AT + from,
            samples: &x[from..],
        });
        v.push(Segment {
            at: EXCERPT_AT + x.len(),
            samples: self.gap(x),
        });
        v
    }
}
