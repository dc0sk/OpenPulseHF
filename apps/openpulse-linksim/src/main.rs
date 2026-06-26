//! Two-station link simulator CLI.
//!
//! Runs one link or an SNR sweep and reports the effective two-way transfer rate —
//! a proof of real-world goodput under simulated noise/fading, accounting for ACK
//! overhead, turnaround, retransmission, and over-the-air rate adaptation.

use clap::{Parser, ValueEnum};
use openpulse_core::compression::{CompressionAlgorithm, ZSTD_DICT_ID};
use openpulse_core::fec::FecMode;
use openpulse_linksim::{run_link, ChannelSpec, LinkParams, LinkResult};

#[derive(Clone, Copy, ValueEnum)]
enum CompressionArg {
    None,
    Lz4,
    Zstd,
}

impl From<CompressionArg> for CompressionAlgorithm {
    fn from(a: CompressionArg) -> Self {
        match a {
            CompressionArg::None => CompressionAlgorithm::None,
            CompressionArg::Lz4 => CompressionAlgorithm::Lz4,
            CompressionArg::Zstd => CompressionAlgorithm::Zstd(ZSTD_DICT_ID),
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum ChannelKind {
    Clean,
    Awgn,
    WattersonGood,
    WattersonModerate,
    WattersonPoor,
    GilbertElliott,
    FlatFading,
}

impl ChannelKind {
    fn spec(self, snr: f32) -> ChannelSpec {
        match self {
            ChannelKind::Clean => ChannelSpec::Clean,
            ChannelKind::Awgn => ChannelSpec::Awgn(snr),
            ChannelKind::WattersonGood => ChannelSpec::WattersonGoodF1(snr),
            ChannelKind::WattersonModerate => ChannelSpec::WattersonModerateF1(snr),
            ChannelKind::WattersonPoor => ChannelSpec::WattersonPoorF1(snr),
            ChannelKind::GilbertElliott => ChannelSpec::GilbertElliott(snr),
            ChannelKind::FlatFading => ChannelSpec::FlatFading(snr),
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum FecArg {
    None,
    Rs,
    RsStrong,
    Soft,
}

impl From<FecArg> for FecMode {
    fn from(a: FecArg) -> Self {
        match a {
            FecArg::None => FecMode::None,
            FecArg::Rs => FecMode::Rs,
            FecArg::RsStrong => FecMode::RsStrong,
            FecArg::Soft => FecMode::SoftConcatenated,
        }
    }
}

#[derive(Parser)]
#[command(
    name = "openpulse-linksim",
    about = "Two-station ARQ link simulator — effective two-way transfer rate under noise",
    author,
    version
)]
struct Cli {
    /// SessionProfile (adaptive ladder) name.
    #[arg(long, default_value = "hpx_hf")]
    profile: String,
    /// Forward (A→B) channel kind.
    #[arg(long, value_enum, default_value = "awgn")]
    channel: ChannelKind,
    /// Reverse (B→A) ACK channel kind (defaults to the forward kind).
    #[arg(long, value_enum)]
    reverse_channel: Option<ChannelKind>,
    /// FEC for data frames.
    #[arg(long, value_enum, default_value = "rs")]
    fec: FecArg,
    /// Payload compression applied before FEC.
    #[arg(long, value_enum, default_value = "none")]
    compression: CompressionArg,
    /// Disable CE-SSB TX envelope conditioning (on by default; only affects OFDM QPSK/8PSK).
    #[arg(long)]
    no_cessb: bool,
    /// Payload bytes per data frame.
    #[arg(long, default_value = "64")]
    payload: usize,
    /// Number of data frames per run.
    #[arg(long, default_value = "40")]
    frames: usize,
    /// Half-duplex turnaround per direction switch (seconds).
    #[arg(long, default_value = "0.25")]
    turnaround: f64,
    /// Max transmission attempts per frame.
    #[arg(long, default_value = "6")]
    max_attempts: u32,
    /// RNG seed.
    #[arg(long, default_value = "49374")]
    seed: u64,
    /// Single-run forward SNR (dB). Ignored when --sweep is given.
    #[arg(long, default_value = "15.0")]
    snr: f32,
    /// Run an SNR sweep "start:stop:step" (dB) instead of a single run.
    #[arg(long)]
    sweep: Option<String>,
    /// Emit JSON instead of a table.
    #[arg(long)]
    json: bool,
    /// Serve the openpulse-daemon control protocol on this address (e.g. 127.0.0.1:9000)
    /// so an unmodified openpulse-panel can connect and visualize a live simulation.
    /// Requires building with `--features serve`.
    #[arg(long, value_name = "ADDR")]
    serve: Option<String>,
    /// Waterfall scroll rate (frames/second) for `--serve`.
    #[arg(long, default_value = "20")]
    serve_fps: u32,
}

fn parse_sweep(s: &str) -> Result<Vec<f32>, String> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 3 {
        return Err("sweep must be start:stop:step".into());
    }
    let start: f32 = parts[0].parse().map_err(|_| "bad start")?;
    let stop: f32 = parts[1].parse().map_err(|_| "bad stop")?;
    let step: f32 = parts[2].parse().map_err(|_| "bad step")?;
    if step <= 0.0 {
        return Err("step must be > 0".into());
    }
    let mut v = Vec::new();
    let mut x = start;
    while x <= stop + 1e-6 {
        v.push(x);
        x += step;
    }
    Ok(v)
}

fn params_for(cli: &Cli, snr: f32) -> LinkParams {
    let reverse_kind = cli.reverse_channel.unwrap_or(cli.channel);
    LinkParams {
        profile_name: cli.profile.clone(),
        forward: cli.channel.spec(snr),
        // Give the ACK path a few dB more headroom than the data path, as is typical.
        reverse: reverse_kind.spec(snr + 5.0),
        payload_bytes_per_frame: cli.payload,
        total_frames: cli.frames,
        fec: cli.fec.into(),
        compression: cli.compression.into(),
        turnaround_s: cli.turnaround,
        max_attempts: cli.max_attempts,
        seed: cli.seed,
        cessb_enabled: !cli.no_cessb,
    }
}

#[cfg(feature = "serve")]
fn serve_mode(cli: &Cli, addr: &str) {
    let mut params = params_for(cli, cli.snr);
    params.total_frames = usize::MAX; // run continuously until the panel disconnects
    if let Err(e) = openpulse_linksim::serve::serve(addr, &params, cli.serve_fps) {
        eprintln!("linksim serve error: {e}");
        std::process::exit(1);
    }
}

#[cfg(not(feature = "serve"))]
fn serve_mode(_cli: &Cli, _addr: &str) {
    eprintln!(
        "--serve requires the `serve` feature: \
         cargo run -p openpulse-linksim --features serve -- --serve <ADDR>"
    );
    std::process::exit(2);
}

fn fmt_bps(bps: f64) -> String {
    if bps >= 1000.0 {
        format!("{:.2} kbps", bps / 1000.0)
    } else {
        format!("{bps:.1} bps")
    }
}

fn print_row(r: &LinkResult) {
    println!(
        "{:>14} | {:>14} | {:>6.0}% | {:>10} | avg SL {:>4.1} | final SL{:<2} | {:>6.1} s",
        r.profile,
        r.forward,
        r.delivery_ratio * 100.0,
        fmt_bps(r.effective_bps),
        r.avg_level,
        r.final_level,
        r.total_air_s,
    );
}

fn main() {
    let cli = Cli::parse();

    if let Some(addr) = cli.serve.clone() {
        serve_mode(&cli, &addr);
        return;
    }

    let snrs = match &cli.sweep {
        Some(s) => match parse_sweep(s) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("--sweep: {e}");
                std::process::exit(2);
            }
        },
        None => vec![cli.snr],
    };

    let results: Vec<LinkResult> = snrs
        .iter()
        .map(|&snr| run_link(&params_for(&cli, snr)))
        .collect();

    if cli.json {
        match serde_json::to_string_pretty(&results) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("json error: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    println!(
        "Two-station link — profile {} | {} frames × {} B | FEC {:?} | turnaround {:.0} ms\n",
        cli.profile,
        cli.frames,
        cli.payload,
        Into::<FecMode>::into(cli.fec),
        cli.turnaround * 1000.0,
    );
    println!(
        "{:>14} | {:>14} | {:>7} | {:>10} | {:>9} | {:>9} | {:>8}",
        "profile", "fwd channel", "deliver", "effective", "avg level", "final", "air time"
    );
    println!("{}", "-".repeat(92));
    for r in &results {
        print_row(r);
    }
}
