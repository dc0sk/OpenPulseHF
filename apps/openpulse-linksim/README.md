# openpulse-linksim

Two-station bidirectional ARQ **link simulator**. It proves the *effective two-way
transfer rate* a station actually achieves under simulated noise / fading — not the raw
modem rate — by running a realistic half-duplex exchange between two stations and
accounting for everything that eats into goodput.

## What it models

- **Station A → B (forward):** real data frames at the current speed level, modulated by a
  real `ModemEngine` and routed through a forward `ChannelModel` realization.
- **Station B → A (reverse):** B decodes, estimates the per-frame SNR, and returns a real
  FSK4 **ACK frame** (`AckOk` / `AckUp` / `AckDown` / `Nack`) through an independent reverse
  channel. A lost/garbled ACK is treated as an implicit NACK.
- **Over-the-air rate adaptation:** A steps the speed level up/down the chosen
  `SessionProfile` ladder from the ACKs (mirroring `RateAdapter` policy, bounded to the
  profile's defined levels).
- **Goodput accounting:** forward air time + ACK air time + turnaround × 2, summed over
  every attempt (including retransmissions). `effective_bps = delivered_bits / total_air`.

This captures the real costs — ACK overhead, half-duplex turnaround, retransmits, and the
cold-start climb up the ladder — that make the effective two-way rate far lower than the
forward mode's gross rate.

## CLI

```bash
# Single run
cargo run -p openpulse-linksim -- --profile hpx_hf --channel awgn --snr 15 --fec rs

# SNR sweep (start:stop:step dB) → effective-rate table
cargo run -p openpulse-linksim -- --profile hpx500 --channel awgn --sweep 0:24:4 --frames 30

# Watterson fading, JSON output
cargo run -p openpulse-linksim -- --profile hpx_hf --channel watterson-moderate --snr 18 --json
```

Channels: `clean`, `awgn`, `watterson-good`, `watterson-moderate`, `watterson-poor`,
`gilbert-elliott`. FEC: `none`, `rs`, `rs-strong`, `soft`.

## GUI (live visualizer)

```bash
cargo run -p openpulse-linksim --features gui --bin openpulse-linksim-gui
```

Side-by-side **Station A | Channel | Station B**: A's clean data TX (left), the noisy
on-air signal (middle), and B's FSK4 ACK response (right) — each with a live spectrum +
waterfall. The toolbar selects the profile / channel / FEC and an **SNR slider that adjusts
the channel live**, so you can watch the ladder adapt and the bottom plot track the
effective two-way transfer rate over time. Runs continuously on a background thread over the
same `LinkSim` engine.

## Serve the operator panel (`openpulse-panel`)

The simulator can speak the **`openpulse-daemon` control protocol**, so an *unmodified*
`openpulse-panel` connects to it exactly as it would to a real station — no daemon, modem,
or audio hardware required:

```bash
# Terminal 1 — run the sim as a fake daemon
cargo run -p openpulse-linksim --features serve -- \
    --serve 127.0.0.1:9000 --profile hpx_hf --channel awgn --snr 12

# Terminal 2 — point the panel at it
cargo run -p openpulse-panel        # Server: 127.0.0.1:9000 → Connect
```

The panel then shows the live simulated link: the speed-level ladder climbing/dropping, HPX
session state, effective-bps / compression / signal metrics, and the on-air waterfall (the
FFT of the post-channel received waveform). It emits the same NDJSON `ControlEvent` stream
interleaved with binary `OPSP` spectrum frames a real daemon does. Operator-only controls
(messages, QSY, rig CAT, PTT, RF-connect) are inert — there is no real peer. `--serve-fps`
paces the waterfall scroll.

## Library

`LinkSim` is a step-able simulation (`new` / `step` → `FrameStep` / `set_conditions` /
`result`); `run_link(&LinkParams) -> LinkResult` runs one to completion. Headless, no audio
hardware. See `LinkParams` / `LinkResult` / `FrameStep` / `ChannelSpec`.
