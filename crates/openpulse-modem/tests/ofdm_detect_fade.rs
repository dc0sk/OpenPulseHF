//! Gate 3 (`docs/dev/design/ofdm-preamble-detect.md`): P gathers, off the frequency grid and through a
//! fade, every SL7 frame the stack can decode — and counts the frames its confirm refused.
//!
//! SL7 (`OFDM52`, soft-concatenated) at 9 dB on Watterson `moderate_f1`, carrier offsets +31.25 and
//! −43.75 Hz (halfway between the ±50 Hz / 12.5 Hz grid's points), embedded in recorded hiss under an
//! AGC: the hiss 12 dB down under each frame, the total 1.15× the gap. Watterson's AWGN is normalised
//! over the frame-only buffer, so SNR and duration are not confounded. Sabotage S3 (the told offset
//! only, no grid) must fail at +31.25 Hz, where ρ falls to about sinc(1.1).
//!
//! Its own binary: the confirm's refusals are counted from the detector's debug lines, which a
//! concurrent test in the same process would mix in.

#[path = "common/ofdm_detect.rs"]
mod fixture;

use std::io::Write;
use std::sync::{Arc, Mutex};

use fixture::*;
use openpulse_channel::watterson::WattersonChannel;
use openpulse_channel::{ChannelModel, WattersonConfig};
use openpulse_core::fec::FecMode;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::pipeline::AudioSamples;

#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<u8>>>);

impl Write for Log {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log").extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Log {
    fn count(&self, needle: &str) -> usize {
        String::from_utf8_lossy(&self.0.lock().expect("log"))
            .matches(needle)
            .count()
    }
}

const REFUSED: &str = "the cyclic prefix refused a preamble candidate";
const PAYLOAD: [u8; 64] = [0x3C; 64];

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

struct Cell {
    offset_hz: f32,
    decodable: usize,
    decodable_seeds: Vec<u64>,
    /// Frames one flushed burst held whole.
    gathered: usize,
    /// Decodable frames the OTA arm decoded from the burst P gathered.
    decoded: usize,
    /// Decodable frames P did not gather.
    missed: Vec<u64>,
    refused_among_decodable: usize,
}

fn run(seeds: std::ops::Range<u64>, offsets: &[f32], log: &Log) -> Vec<Cell> {
    let tx = {
        let lb = openpulse_audio::LoopbackBackend::new();
        let mut t = openpulse_modem::ModemEngine::new(Box::new(lb.clone_shared()));
        t.register_plugin(Box::new(ofdm_plugin::OfdmPlugin::new()))
            .expect("ofdm");
        t.transmit_with_fec_mode(&PAYLOAD, "OFDM52", FecMode::SoftConcatenated, None)
            .expect("tx");
        lb.drain_samples()
    };
    let hiss = capture("ic9700-idle-wide-500hz-control.wav");
    let h = rms(&hiss);
    let sym = ofdm_symbol();
    let mut cells = Vec::new();
    for &hz in offsets {
        let shifted = ofdm_plugin::shift::shift_hz(&tx, hz, RATE as f32);
        let mut cell = Cell {
            offset_hz: hz,
            decodable: 0,
            decodable_seeds: Vec::new(),
            gathered: 0,
            decoded: 0,
            missed: Vec::new(),
            refused_among_decodable: 0,
        };
        // One engine across the cell's frames, as a daemon hears them: 10 s of hiss to warm the floor,
        // then frame, 4 s of hiss, frame, …
        let mut e = engine(SpeedLevel::Sl7, true);
        let warm = &hiss[..s(10.0)];
        let _ = feed(
            &mut e,
            &[Segment {
                at: 0,
                samples: warm,
            }],
            800,
            false,
        );
        let mut at = s(10.0);
        for seed in seeds.clone() {
            let mut cfg = WattersonConfig::moderate_f1(Some(9_000 + seed));
            cfg.snr_db = 9.0;
            let y = WattersonChannel::new(cfg).expect("channel").apply(&shifted);
            let under = 0.25f32;
            let g = ((1.15 * 1.15 - under * under) * h * h).sqrt() / rms(&y);
            let gap = s(4.0);
            let mut x: Vec<f32> = (0..y.len() + gap)
                .map(|i| hiss[(at + i) % hiss.len()])
                .collect();
            for (i, v) in y.iter().enumerate() {
                x[i] = x[i] * under + v * g;
            }
            // What the stack can decode, handed the frame's own window.
            let window = AudioSamples {
                samples: hiss[s(20.0)..s(20.0) + sym]
                    .iter()
                    .chain(&x[..y.len() + s(0.5)])
                    .copied()
                    .collect(),
            };
            let mut d = engine(SpeedLevel::Sl7, true);
            let decodable = d
                .decode_burst_with_fec("OFDM52", FecMode::SoftConcatenated, &window)
                .is_ok_and(|p| p == PAYLOAD);
            let refused_before = log.count(REFUSED);
            let flushes = feed(&mut e, &[Segment { at, samples: &x }], 800, true);
            let refused = log.count(REFUSED) > refused_before;
            // Gathered: one flushed burst holds the whole frame.
            let holding = flushes
                .iter()
                .find(|f| f.start <= at && f.start + f.len >= at + y.len());
            let got = holding.is_some();
            let decoded =
                holding.is_some_and(|f| f.decoded.as_ref().is_some_and(|(_, p)| p == &PAYLOAD));
            if decodable {
                cell.decodable += 1;
                cell.decodable_seeds.push(seed);
                if refused {
                    cell.refused_among_decodable += 1;
                }
                if !got {
                    cell.missed.push(seed);
                }
                if decoded {
                    cell.decoded += 1;
                }
            }
            if got {
                cell.gathered += 1;
            }
            at += x.len();
        }
        cells.push(cell);
    }
    cells
}

fn check(cells: &[Cell]) {
    for c in cells {
        println!(
            "{:+.2} Hz: decodable {} (seeds {:?}); gathered whole {} of all; decodable and decoded \
             from P's burst {}; decodable but not gathered {:?}; confirm refusals among the \
             decodable {}",
            c.offset_hz,
            c.decodable,
            c.decodable_seeds,
            c.gathered,
            c.decoded,
            c.missed,
            c.refused_among_decodable
        );
    }
    for c in cells {
        assert!(
            c.decodable > 0,
            "{:+.2} Hz: no frame was decodable even by hand, so this cell proves nothing",
            c.offset_hz
        );
        assert!(
            c.missed.is_empty(),
            "{:+.2} Hz: P did not gather {} frame(s) the stack decodes when handed them (seeds {:?})",
            c.offset_hz,
            c.missed.len(),
            c.missed
        );
    }
}

/// One subscriber for the process, shared by whichever tests run in it.
fn subscriber() -> Log {
    static LOG: std::sync::OnceLock<Log> = std::sync::OnceLock::new();
    LOG.get_or_init(|| {
        let log = Log::default();
        let sink = log.clone();
        tracing_subscriber::fmt()
            .with_env_filter("ofdm_plugin=debug")
            .with_writer(move || sink.clone())
            .with_ansi(false)
            .init();
        log
    })
    .clone()
}

/// The default run: three fades at each offset, two of which the stack decodes at both (seeds 7 and
/// 9; measured by the 40-frame run below).
#[test]
fn ofdm_frame_gathers_off_grid_and_faded() {
    let log = subscriber();
    check(&run(7..10, &[31.25, -43.75], &log));
}

/// The measurement the design note sizes: 20 fades at each offset (40 frames).
#[test]
#[ignore = "runtime: scripts/slow-tests.sh spectral"]
fn ofdm_frame_gathers_off_grid_and_faded_40_frames() {
    let log = subscriber();
    check(&run(0..20, &[31.25, -43.75], &log));
}
